use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::oneshot::{Receiver, Sender};
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
pub struct ProbeServices {
    consul_client: ConsulClient,
    tag: String,
    interval_check_ms: u64,
    probe_nodes: HashMap<String, Sender<u8>>,
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
                probe_nodes_to_stop.push(probe_node_key.clone());
            }
        }

        for probe_node_to_stop in probe_nodes_to_stop.iter() {
            info!("Stop to probe node: {}", probe_node_to_stop);
            match self.probe_nodes.remove(probe_node_to_stop) {
                Some(stop_probe_resp_tx) => {
                    stop_probe_resp_tx.send(1).unwrap_or(());
                }
                None => warn!("Node {} is not a monitored node", probe_node_to_stop),
            }
        }
    }

    /// Remove all prometheus metrics of that memcached node
    ///
    /// # Arguments
    ///
    /// * `service_name` - name of the memcached service
    /// * `addr` - socker of the memcached instance monitored
    ///
    fn stop_node(service_name: String, addr: String) {
        FAILURE_PROBE
            .remove_label_values(&[service_name.as_str(), addr.as_str()])
            .unwrap_or(());

        for cmd_type in ["set", "get"] {
            RESPONSE_TIME_COLLECTOR
                .remove_label_values(&[service_name.as_str(), addr.as_str(), cmd_type])
                .unwrap_or(());

            for status in STATUS_CODE.keys() {
                NUMBER_OF_REQUESTS
                    .remove_label_values(&[
                        service_name.as_str(),
                        addr.as_str(),
                        STATUS_CODE.get(status).unwrap(),
                        cmd_type,
                    ])
                    .unwrap_or(());
            }
        }
    }

    /// The memcached probe
    /// Manage connection to the memcached
    /// Check if any message have been send on the stop_probe_resp channel
    /// If it is the case remove all related prometheus metrics and break the probe loop
    ///
    /// # Arguments
    ///
    /// * `service_name` - name of the memcached service
    /// * `addr` - socker of the memcached instance monitored
    /// * `interval_check_ms` - interval between each check
    /// * `stop_probe_resp_rx` - receiver for stop probe channel dedicated to that probe
    ///
    async fn start_node_probe(
        service_name: String,
        addr: String,
        interval_check_ms: u64,
        mut stop_probe_resp_rx: Receiver<u8>,
    ) {
        let mut c_memcache = memcached::connect(service_name.clone(), addr.clone())
            .await
            .unwrap();
        loop {
            match stop_probe_resp_rx.try_recv() {
                Ok(_) => {
                    ProbeServices::stop_node(service_name, addr);
                    break;
                }
                Err(TryRecvError::Empty) => {
                    c_memcache.probe().await;
                    // TODO manage failure like connection break to be able to recreate connection stop + send message to main process
                }
                Err(TryRecvError::Closed) => {
                    ProbeServices::stop_node(service_name, addr);
                    break;
                }
            }
            sleep(Duration::from_millis(interval_check_ms)).await;
        }
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
            let key_node = discovered_node.0.clone();
            let service_node = discovered_node.1;
            if !self.probe_nodes.contains_key(key_node.as_str()) {
                info!("Start to probe node: {}", key_node);

                let (stop_probe_resp_tx, stop_probe_resp_rx) = oneshot::channel();
                self.probe_nodes.insert(key_node, stop_probe_resp_tx);

                tokio::spawn(ProbeServices::start_node_probe(
                    service_node.service_name.clone(),
                    format!("{}:{}", service_node.ip.clone(), service_node.port).clone(),
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
        let mut token_bucket = TokenBucket::new(60, 1);
        let mut index = 0;

        loop {
            token_bucket.wait_for(60).await?;

            match self
                .consul_client
                .list_matching_nodes(index, self.tag.clone())
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
