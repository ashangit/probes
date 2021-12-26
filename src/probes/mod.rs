use std::collections::HashMap;

use log::{debug, error, info, warn};
use tokio::sync::oneshot;
use tokio::sync::oneshot::{Receiver, Sender};

use crate::consul::ConsulClient;
use crate::token_bucket::TokenBucket;

pub struct ProbeServices {
    consul_client: ConsulClient,
    tag: String,
    watch_services: HashMap<String, Sender<u8>>,
}

impl ProbeServices {
    pub fn new(consul_client: ConsulClient, tag: String) -> ProbeServices {
        debug!("Create a probe for services with tag {}", tag);
        ProbeServices {
            consul_client,
            tag,
            watch_services: HashMap::new(),
        }
    }

    fn stop_watch_service(&mut self, matching_services: &Vec<String>) {
        let mut services_to_remove: Vec<String> = Vec::new();
        for service_name in self.watch_services.keys() {
            if !matching_services.contains(service_name) {
                services_to_remove.push(service_name.clone());
            }
        }

        for service_name in services_to_remove.iter() {
            // TODO call queue to stop async of service
            debug!("Stop watching service {}", service_name);
            match self.watch_services.remove(service_name) {
                Some(resp_tx) => {
                    resp_tx.send(1);
                }
                None => warn!("Service {} is not a watched services", service_name),
            }
        }
    }

    fn add_watch_service(&mut self, matching_services: &Vec<String>) {
        for service_name in matching_services.iter() {
            if !self.watch_services.contains_key(service_name) {
                debug!("Start to watch service {}", service_name);
                let (resp_tx, resp_rx) = oneshot::channel();
                self.watch_services.insert(service_name.clone(), resp_tx);
                let cc = self.consul_client.clone();
                let s = service_name.clone();
                tokio::spawn(async move {
                    let mut probe_service = ProbeService::new(s, cc, resp_rx);
                    probe_service.watch_service().await;
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

            // TODO manage error here? failed prog or just display error?
            match self
                .consul_client
                .list_matching_services(index, self.tag.clone())
                .await
            {
                Ok(matching_services) => {
                    index = matching_services.index;

                    self.stop_watch_service(&matching_services.services);
                    self.add_watch_service(&matching_services.services);
                }
                Err(err) => {
                    error!("Failed to get list of matching services: {}", err);
                }
            };
        }
    }
}

pub struct ProbeService {
    service_name: String,
    consul_client: ConsulClient,
    resp_rx: Receiver<u8>,
    watch_nodes: HashMap<String, Sender<u8>>,
}

impl ProbeService {
    pub fn new(
        service_name: String,
        consul_client: ConsulClient,
        resp_rx: Receiver<u8>,
    ) -> ProbeService {
        debug!("Create a probe for service {}", service_name);
        ProbeService {
            service_name,
            consul_client,
            resp_rx,
            watch_nodes: HashMap::new(),
        }
    }

    // TODO create memcached client with set and get (https://www.slideshare.net/tmaesaka/memcached-binary-protocol-in-a-nutshell-presentation)
    pub async fn watch_service(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Start watching service {}", self.service_name);
        let mut token_bucket = TokenBucket::new(60, 1);
        let mut index = 0;

        loop {
            token_bucket.wait_for(60).await?;

            // TODO manage error here? failed prog or just display error?
            match self
                .consul_client
                .list_nodes_for_service(index, self.service_name.clone())
                .await
            {
                Ok(service_nodes) => {
                    // TODO init one async task per service found if not in vec with an smp channel to send msg
                    //   send stop to channel if not in service list but in vec (del from vec)
                    index = service_nodes.index;

                    // TODO watch for list of nodes and get external fqdn?
                }
                Err(err) => {
                    error!("Failed to get list of matching services: {}", err);
                }
            };
        }
    }
}
