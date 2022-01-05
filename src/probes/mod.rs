use std::collections::HashMap;

use std::fmt::Debug;

use tokio::sync::oneshot;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::oneshot::Sender;
use tracing::log::warn;
use tracing::{debug, error, info};

use crate::consul::{ConsulClient, ServiceNode};
use crate::memcached;
use crate::probes::prometheus::FAILURE_SERVICES_DISCOVERY;
use crate::token_bucket::TokenBucket;

pub mod prometheus;

pub async fn init_probing(
    services_tag: String,
    consul_fqdn: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let consul_client = ConsulClient::new(consul_fqdn);
    let mut probe = ProbeServices::new(consul_client, services_tag);
    probe.watch_matching_services().await?;
    Ok(())
}

#[derive(Debug)]
pub struct ProbeServices {
    consul_client: ConsulClient,
    tag: String,
    probe_nodes: HashMap<String, Sender<u8>>,
}

impl ProbeServices {
    pub fn new(consul_client: ConsulClient, tag: String) -> ProbeServices {
        debug!("Create a probe for services with tag {}", tag);
        ProbeServices {
            consul_client,
            tag,
            probe_nodes: HashMap::new(),
        }
    }

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
                Some(resp_tx) => {
                    resp_tx.send(1).unwrap_or(());
                }
                None => warn!("Node {} is not a monitored node", probe_node_to_stop),
            }
        }
    }

    fn start_nodes_probe(&mut self, discovered_nodes: &HashMap<String, ServiceNode>) {
        for discovered_node in discovered_nodes.iter() {
            let key_node = discovered_node.0.clone();
            let service_node = discovered_node.1;
            if !self.probe_nodes.contains_key(key_node.as_str()) {
                info!("Start to probe node: {}", key_node);

                let (resp_tx, mut resp_rx) = oneshot::channel();
                self.probe_nodes.insert(key_node, resp_tx);

                let task_service_name = service_node.service_name.clone();
                let addr = format!("{}:{}", service_node.ip.clone(), service_node.port).clone();

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
