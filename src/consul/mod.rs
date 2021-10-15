use hyper::{Body, Client};
use hyper::client::HttpConnector;
use log::{debug, error, warn};
use serde_json::Value;

use crate::token_bucket::TokenBucket;

// Represent a consul client
pub struct ConsulClient {
    // The fqdn of the consul agent to query
    fqdn: String,
    // Services tag to search on consul
    tag: String,
}

pub struct WatchedServices {
    index: i64,
    services: Vec<String>,
}

impl ConsulClient {
    /// Returns a consul client
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname of the consul agent
    /// * `port` - Port of the consul agent
    /// * `tag` - Tag of the service we want to extract
    ///
    /// # Examples
    ///
    /// ```
    /// use probes::consul::ConsulClient;
    /// let mut consul_client = ConsulClient::new("localhost".to_string(), 8500, "tag".to_string());
    /// ```
    pub fn new(hostname: String, port: u64, tag: String) -> ConsulClient {
        debug!("Create consul client {}:{} filtering on tag {}", hostname, port, tag);
        ConsulClient {
            fqdn: format!("{}:{}", hostname, port),
            tag: tag,
        }
    }

    fn get_string_value(&mut self, value: &Value) -> String {
        match value.as_str() {
            Some(x) => x.to_string(),
            None => "".to_string(),
        }
    }

    fn is_matching_service(&mut self, tags_opt: Option<&Vec<Value>>) -> bool {
        match tags_opt {
            Some(tags) => {
                if tags.iter().map(|value| self.get_string_value(value)).collect::<Vec<String>>().contains(&self.tag) {
                    return true;
                }
            }
            None => ()
        }

        return false;
    }

    fn get_matching_services(&mut self, body_str: String) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let deserialized_body: Value = serde_json::from_str(body_str.as_str())?;

        let matching_services = deserialized_body.as_object()
            .unwrap()
            .keys()
            .filter(|&key| self.is_matching_service(deserialized_body[key].as_array()))
            .cloned()
            .collect::<Vec<String>>();

        debug!("Services matching tag {}: {}", self.tag, matching_services.join(", "));

        Ok(matching_services)
    }

    async fn get_services(&mut self, client: &Client<HttpConnector, Body>, prev_index: i64) -> Result<Option<WatchedServices>, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/services?index={}", self.fqdn, prev_index);

        debug!("Query for list of services at index {}", prev_index);
        let resp = client.get(services_uri.as_str().parse()?).await?;

        if !resp.status().is_success() {
            error!("Failed to get list of service, http status code {}", resp.status());
            return Err(format!("Issue query: {} - status code: {}", services_uri, resp.status()).into());
        }

        let (parts, body) = resp.into_parts();

        let resp_index: i64 = parts.headers.get("x-consul-index").unwrap().to_str().unwrap().parse()?;

        // TODO check with consul doc which condition should lead to reset
        if resp_index < prev_index {
            warn!("Consul index querying list of services is lower than previous one. Will need to reset it to 0");
            return Ok(None);
        }

        let bytes = hyper::body::to_bytes(body).await?;
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();
        let matching_services = self.get_matching_services(body_str);

        Ok(Some(WatchedServices { index: resp_index, services: matching_services.unwrap() }))
    }

    pub async fn watch_services(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = Client::new();
        let mut index = 0;

        let mut token_bucket = TokenBucket::new(60, 1);

        loop {
            token_bucket.wait_for(60).await?;

            let watch_services = self.get_services(&client, index).await?;

            match watch_services {
                Some(ws) => index = ws.index,
                None => index = 0,
            }

            if index < 0 {
                error!("Consul index {} < 0. Failing...", index);
                break;
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::consul::ConsulClient;
    use serde_json::Value;
    use hyper::Body;

    #[test]
    fn get_string_value() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1, "elasticsearch".to_string());
        assert_eq!(consul_client.get_string_value(&Value::String("test".to_string())), "test".to_string());
        assert_eq!(consul_client.get_string_value(&Value::Null), "".to_string());
    }

    #[test]
    fn is_matching_service() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1, "elasticsearch".to_string());
        assert_eq!(consul_client.is_matching_service(Some(&vec![Value::String("elasticsearch".to_string()), Value::String("http".to_string())])), true);
        assert_eq!(consul_client.is_matching_service(Some(&vec![Value::String("memcached".to_string()), Value::String("tcp".to_string())])), false);
        assert_eq!(consul_client.is_matching_service(None), false);
    }

    #[test]
    fn get_matching_services() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1, "elasticsearch".to_string());

        let body_str = "{\"youfollow-yourequest-admin\":[\"netcore\",\"73c7b23f2ce611ecb9d488e9a4060640\",\"admin-handler-api\",\
        \"default\",\"http\",\"marathon\",\"marathon-start-20211014T120126Z\",\"marathon-user-svc-youfollow\"],\
        \"elasticsearch-secauditlogs-https\":[\"https\",\"elasticsearch\",\"master\",\"data\",\"cluster_name-secauditlogs\",\"version-7.7.1\",\"maintenance-elasticsearch\",\"nosql\"],\
        \"elasticsearch-shared\":[\"nosql\",\"data\",\"cluster_name-shared-s01\",\"version-6.8.10\",\"\",\"https\",\"elasticsearch\",\"master\",\"maintenance-elasticsearch\"]}".to_string();
        assert_eq!(consul_client.get_matching_services(body_str).unwrap(), vec!["elasticsearch-secauditlogs-https", "elasticsearch-shared"]);
    }
}