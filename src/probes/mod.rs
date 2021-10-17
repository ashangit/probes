use log::debug;

use crate::token_bucket::TokenBucket;
use std::collections::HashMap;
use crate::consul::ConsulClient;

// Represent a consul client
pub struct Probe {
    // The fqdn of the consul agent to query
    consul_client: ConsulClient,
    // Services tag to search on consul
    tag: String,
    //
    watch_services: HashMap<String, String>,
}

impl Probe {
    pub fn new(consul_client: ConsulClient, tag: String) -> Probe {
        debug!("Create a probe for services with tag {}", tag);
        Probe {
            consul_client: consul_client,
            tag: tag,
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
            self.watch_services.remove(service_name);
        }
    }

    fn add_watch_service(&mut self, matching_services: &Vec<String>) {
        for service_name in matching_services.iter() {
            if !self.watch_services.contains_key(service_name) {
                debug!("Start to watch service {}", service_name);
            }
        }
    }

    pub async fn watch_matching_services(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut token_bucket = TokenBucket::new(60, 1);
        let mut index = 0;

        loop {
            token_bucket.wait_for(60).await?;

            // TODO manage error here? failed prog or just display error?
            let res = self.consul_client.get_matching_services(index, self.tag.clone()).await?;

            // TODO init one async task per service found if not in vec with an smp channel to send msg
            //   send stop to channel if not in service list but in vec (del from vec)
            match res {
                Some(matching_services) => {
                    index = matching_services.index;

                    self.stop_watch_service(&matching_services.services);
                    self.add_watch_service(&matching_services.services);
                }
                None => index = 0,
            }
        };
    }
}