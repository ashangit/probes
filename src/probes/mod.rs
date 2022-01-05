use std::collections::HashMap;

use log::{debug, error, warn};
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::oneshot::Sender;

use crate::consul::{ConsulClient, ServiceNode};
use crate::memcached;
use crate::probes::prometheus::FAILURE_SERVICES_DISCOVERY;
use crate::token_bucket::TokenBucket;

pub mod prometheus;

pub async fn init_probing(services_tag: String, consul_fqdn: String) {
    let consul_client = ConsulClient::new(consul_fqdn);
    let mut probe = ProbeServices::new(consul_client, services_tag);
    probe.watch_matching_services().await;
}

pub struct ProbeServices {
    consul_client: ConsulClient,
    tag: String,
    watch_nodes: HashMap<String, Sender<u8>>,
}

impl ProbeServices {
    pub fn new(consul_client: ConsulClient, tag: String) -> ProbeServices {
        debug!("Create a probe for services with tag {}", tag);
        ProbeServices {
            consul_client,
            tag,
            watch_nodes: HashMap::new(),
        }
    }

    fn stop_nodes_probe(&mut self, matching_nodes: &[ServiceNode]) {
        let mut nodes_to_stop: Vec<String> = Vec::new();
        for node in self.watch_nodes.keys() {
            // TODO improve the comparison
            if !matching_nodes
                .iter()
                .map(|node_name| format!("{}:{}", node_name.ip.clone(), node_name.port))
                .any(|x| x == *node)
            {
                nodes_to_stop.push(node.clone());
            }
        }

        for node in nodes_to_stop.iter() {
            debug!("Stop monitoring node {}", node);
            match self.watch_nodes.remove(node) {
                Some(resp_tx) => {
                    resp_tx.send(1);
                }
                None => warn!("Node {} is not a monitored node", node),
            }
        }
    }

    fn start_nodes_probe(&mut self, matching_nodes: &[ServiceNode]) {
        for node_name in matching_nodes.iter() {
            if !self.watch_nodes.contains_key(node_name.ip.as_str()) {
                debug!("Start to probe node {}", node_name.ip);
                let (resp_tx, mut resp_rx) = oneshot::channel();
                let task_service_name = node_name.service_name.clone();
                let addr = format!("{}:{}", node_name.ip.clone(), node_name.port).clone();
                self.watch_nodes.insert(addr.clone(), resp_tx);
                tokio::spawn(async move {
                    let mut c_memcache = memcached::connect(task_service_name, addr).await.unwrap();
                    loop {
                        c_memcache.probe().await;
                        match resp_rx.try_recv() {
                            Ok(_) => {
                                // Manage stop
                                // break loop + remove metrics
                                break;
                            }
                            Err(TryRecvError::Empty) => {
                                // continue
                            }
                            Err(TryRecvError::Closed) => {
                                // Manage stop
                                // break loop + remove metrics
                                break;
                            }
                        }
                    }
                });
            }
        }
    }

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
                Ok(matching_nodes) => {
                    index = matching_nodes.index;

                    self.stop_nodes_probe(&matching_nodes.nodes);
                    self.start_nodes_probe(&matching_nodes.nodes);
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
