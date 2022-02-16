use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::time::sleep;
use tracing::log::warn;
use tracing::{debug, error, info};

use crate::consul::{ConsulClient, ServiceNode};
use crate::memcached;
use crate::memcached::STATUS_CODE;
use crate::probes::prometheus::{
    FAILURE_PROBE, FAILURE_SERVICES_DISCOVERY, NUMBER_OF_REQUESTS, RESPONSE_TIME_COLLECTOR,
};
use crate::token_bucket::TokenBucket;

pub mod prometheus;

pub async fn init_probing(
    services_tag: String,
    consul_fqdn: String,
    interval_check_ms: u64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let consul_client = ConsulClient::new(consul_fqdn);
    let mut probe = ProbeServices::new(consul_client, services_tag, interval_check_ms);
    probe.watch_matching_services().await?;
    Ok(())
}

#[derive(Debug)]
pub struct ProbeNode {
    cluster_name: String,
    ip: String,
    port: u16,
    socket: String,
    interval_check_ms: u64,
    stop_probe_resp_rx: oneshot::Receiver<u8>,
}

impl ProbeNode {
    fn new(
        cluster_name: String,
        ip: String,
        port: u16,
        interval_check_ms: u64,
        stop_probe_resp_rx: oneshot::Receiver<u8>,
    ) -> Self {
        let socket = format!("{}:{}", ip, port);
        ProbeNode {
            cluster_name,
            ip,
            port,
            socket,
            interval_check_ms,
            stop_probe_resp_rx,
        }
    }

    /// Remove all prometheus metrics of that memcached node
    ///
    fn stop(&mut self) {
        FAILURE_PROBE
            .remove_label_values(&[self.cluster_name.as_str(), self.socket.as_str()])
            .unwrap_or(());

        for cmd_type in ["set", "get"] {
            RESPONSE_TIME_COLLECTOR
                .remove_label_values(&[self.cluster_name.as_str(), self.socket.as_str(), cmd_type])
                .unwrap_or(());

            for status in STATUS_CODE.keys() {
                NUMBER_OF_REQUESTS
                    .remove_label_values(&[
                        self.cluster_name.as_str(),
                        self.socket.as_str(),
                        STATUS_CODE.get(status).unwrap(),
                        cmd_type,
                    ])
                    .unwrap_or(());
            }
        }
    }

    fn manage_failure(&mut self, issue: Box<dyn std::error::Error + Send + Sync>) {
        FAILURE_PROBE
            .with_label_values(&[self.cluster_name.as_str(), self.socket.as_str()])
            .inc();
        error!("Failed to probe {} due to {}", self.to_string(), issue);
    }

    /// The memcached probe
    /// Manage connection to the memcached
    /// Check if any message have been send on the stop_probe_resp channel
    /// If it is the case remove all related prometheus metrics and break the probe loop
    ///
    /// # Arguments
    ///
    /// * `interval_check_ms` - interval between each check
    /// * `stop_probe_resp_rx` - receiver for stop probe channel dedicated to that probe
    ///
    async fn start(&mut self) {
        loop {
            match memcached::connect(&self.cluster_name, &self.socket).await {
                Ok(mut c_memcache) => loop {
                    match self.stop_probe_resp_rx.try_recv() {
                        Ok(_) | Err(TryRecvError::Closed) => {
                            info!("Stop to probe node: {}:{}", self.cluster_name, self.socket);
                            return self.stop();
                        }
                        Err(TryRecvError::Empty) => {
                            if let Err(issue) = c_memcache.probe().await {
                                self.manage_failure(issue);
                                break;
                            }
                        }
                    }
                    sleep(Duration::from_millis(self.interval_check_ms)).await;
                },
                Err(issue) => {
                    self.manage_failure(issue);
                }
            }
            match self.stop_probe_resp_rx.try_recv() {
                Ok(_) | Err(TryRecvError::Closed) => {
                    info!("Stop to probe node: {}:{}", self.cluster_name, self.socket);
                    return self.stop();
                }
                Err(TryRecvError::Empty) => {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }
}

impl fmt::Display for ProbeNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.cluster_name, self.ip, self.port)
    }
}

#[derive(Debug)]
pub struct ProbeServices {
    consul_client: ConsulClient,
    tag: String,
    interval_check_ms: u64,
    probe_nodes: HashMap<String, oneshot::Sender<u8>>,
}

impl ProbeServices {
    /// Returns a ProbeServices
    /// Used to manage probes of services/nodes
    ///
    /// # Arguments
    ///
    /// * `consul_client` - a consul client
    /// * `tag` - tag needed on service to enable probing
    /// * `interval_check_ms` - interval between each check
    ///
    ///
    pub fn new(consul_client: ConsulClient, tag: String, interval_check_ms: u64) -> ProbeServices {
        debug!("Create a probe for services with tag {}", tag);
        ProbeServices {
            consul_client,
            tag,
            interval_check_ms,
            probe_nodes: HashMap::new(),
        }
    }

    /// Stop probing nodes that are not part of newly discovered nodes
    ///
    /// # Arguments
    ///
    /// * `discovered_nodes` - hash of new nodes discovered in consul with matching tag
    ///
    fn stop_nodes_probe(&mut self, discovered_nodes: &HashMap<String, ServiceNode>) {
        let mut probe_nodes_to_stop: Vec<String> = Vec::new();
        for probe_node_key in self.probe_nodes.keys() {
            if !discovered_nodes.contains_key(probe_node_key) {
                probe_nodes_to_stop.push(probe_node_key.to_string());
            }
        }

        for probe_node_to_stop in probe_nodes_to_stop.iter() {
            info!("Request to stop to probe node: {}", probe_node_to_stop);
            match self.probe_nodes.remove(probe_node_to_stop) {
                Some(stop_probe_resp_tx) => {
                    stop_probe_resp_tx.send(1).unwrap_or(());
                }
                None => warn!("Node {} is not a monitored node", probe_node_to_stop),
            }
        }
    }

    async fn start_node_probe(
        service_node: ServiceNode,
        interval_check_ms: u64,
        stop_probe_resp_rx: oneshot::Receiver<u8>,
    ) {
        ProbeNode::new(
            service_node.service_name,
            service_node.ip,
            service_node.port,
            interval_check_ms,
            stop_probe_resp_rx,
        )
        .start()
        .await;
    }

    /// Start probing new nodes from newly discovered nodes
    /// Only nodes for which no probes is already running are started
    ///
    /// # Arguments
    ///
    /// * `discovered_nodes` - hash of new nodes discovered in consul with matching tag
    ///
    fn start_nodes_probe(&mut self, discovered_nodes: &HashMap<String, ServiceNode>) {
        for discovered_node in discovered_nodes.iter() {
            let key_node = discovered_node.0.as_str();
            let service_node = discovered_node.1;
            if !self.probe_nodes.contains_key(key_node) {
                info!("Start to probe node: {}", key_node);

                let (stop_probe_resp_tx, stop_probe_resp_rx) = oneshot::channel();
                self.probe_nodes
                    .insert(key_node.to_string(), stop_probe_resp_tx);

                tokio::spawn(ProbeServices::start_node_probe(
                    (*service_node).clone(),
                    self.interval_check_ms,
                    stop_probe_resp_rx,
                ));
            }
        }
    }

    /// Manage services/nodes discovery from consul
    /// and call for probes to stop and add
    pub async fn watch_matching_services(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut token_bucket = TokenBucket::new(180, 1);
        let mut index = 0;

        loop {
            token_bucket.wait_for(60).await?;

            match self
                .consul_client
                .list_matching_nodes(index, &self.tag)
                .await
            {
                Ok(discovered_nodes) => {
                    index = discovered_nodes.index;

                    self.start_nodes_probe(&discovered_nodes.nodes);
                    self.stop_nodes_probe(&discovered_nodes.nodes);
                }
                Err(err) => {
                    index = 0;

                    FAILURE_SERVICES_DISCOVERY.inc();
                    error!("Failed to sync services: {}", err);
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;
    use tokio::sync::oneshot::Sender;

    use crate::probes::prometheus::{FAILURE_PROBE, NUMBER_OF_REQUESTS};
    use crate::probes::ProbeNode;

    fn return_error() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("issue".into())
    }

    fn get_probe() -> (ProbeNode, Sender<u8>) {
        let (stop_probe_resp_tx, stop_probe_resp_rx) = oneshot::channel();

        (
            ProbeNode::new(
                "cluster_name".to_string(),
                "ip".to_string(),
                0,
                1,
                stop_probe_resp_rx,
            ),
            stop_probe_resp_tx,
        )
    }

    #[test]
    fn probe_node_stop() {
        NUMBER_OF_REQUESTS
            .with_label_values(&["cluster_name", "ip:0", "NoError", "get"])
            .inc();

        assert_eq!(
            1,
            NUMBER_OF_REQUESTS
                .get_metric_with_label_values(&["cluster_name", "ip:0", "NoError", "get",])
                .unwrap()
                .get()
        );

        get_probe().0.stop();

        assert_eq!(
            0,
            NUMBER_OF_REQUESTS
                .get_metric_with_label_values(&["cluster_name", "ip:0", "NoError", "get"])
                .unwrap()
                .get()
        );
    }

    #[test]
    fn probe_manage_failure() {
        assert_eq!(
            0,
            FAILURE_PROBE
                .get_metric_with_label_values(&["cluster_name", "ip:0"])
                .unwrap()
                .get()
        );
        get_probe().0.manage_failure(return_error().err().unwrap());

        assert_eq!(
            1,
            FAILURE_PROBE
                .get_metric_with_label_values(&["cluster_name", "ip:0"])
                .unwrap()
                .get()
        );
    }
}
