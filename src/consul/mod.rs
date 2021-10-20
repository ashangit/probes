use hyper::{Client, Uri};
use hyper::client::HttpConnector;
use log::{debug, error, warn};
use serde_json::{Map, Value};

// Represent a consul client
#[derive(Debug, Clone)]
pub struct ConsulClient {
    // The fqdn of the consul agent to query
    fqdn: String,
    client: Client<HttpConnector>,
}

pub struct WatchedServices {
    pub index: i64,
    pub services: Vec<String>,
}

pub struct ServiceNode {
    pub ip: String,
    pub port: i64,
}

pub struct ServiceNodes {
    pub index: i64,
    pub nodes: Vec<ServiceNode>,
}

struct HttpCall {
    index: i64,
    body: String,
}

impl ConsulClient {
    /// Returns a consul client
    ///
    /// # Arguments
    ///
    /// * `hostname` - Hostname of the consul agent
    /// * `port` - Port of the consul agent
    ///
    /// # Examples
    ///
    /// ```
    /// use probes::consul::ConsulClient;
    /// let mut consul_client = ConsulClient::new("localhost".to_string(), 8500);
    /// ```
    pub fn new(hostname: String, port: u64) -> ConsulClient {
        debug!("Create consul client {}:{}", hostname, port);
        ConsulClient {
            fqdn: format!("{}:{}", hostname, port),
            client: Client::new(),
        }
    }

    fn get_string_value(&mut self, value: &Value) -> String {
        match value.as_str() {
            Some(x) => x.to_string(),
            None => "".to_string(),
        }
    }

    fn is_matching_service(&mut self, tag: &String, tags_opt: Option<&Vec<Value>>) -> bool {
        match tags_opt {
            Some(tags) => {
                if tags.iter().map(|value| self.get_string_value(value)).collect::<Vec<String>>().contains(tag) {
                    return true;
                }
            }
            None => ()
        }
        return false;
    }

    fn extract_matching_services(&mut self, tag: &String, body_str: String) -> Vec<String> {
        let deserialized_body: Value = match serde_json::from_str(body_str.as_str()) {
            Err(_) => {
                warn!("Body for services in consul catalog is not json parsable: {}", body_str);
                return Vec::new();
            }
            Ok(x) => x,
        };

        let empty = Map::new();
        let services = match deserialized_body.as_object() {
            Some(x) => x,
            None => {
                warn!("Empty list of services on the consul catalog");
                &empty
            }
        };

        let matching_services = services
            .keys()
            .filter(|&key| self.is_matching_service(tag, deserialized_body[key].as_array()))
            .cloned()
            .collect::<Vec<String>>();

        debug!("Services matching tag {}: {}", tag, matching_services.join(", "));
        matching_services
    }

    async fn http_call(&mut self, uri_str: String, prev_index: i64) -> Result<Option<HttpCall>, Box<dyn std::error::Error + Send + Sync>> {
        debug!("Query consul: {}", uri_str);
        let uri = match uri_str.as_str().parse::<Uri>() {
            Err(issue) => {
                error!("Invalid uri {} -> {}", uri_str, issue.to_string());
                return Err(issue.into());
            }
            Ok(_uri) => _uri,
        };

        let resp = self.client.get(uri).await?;

        if !resp.status().is_success() {
            error!("Failed to query consul, http status code {}", resp.status());
            return Err(format!("Issue query: {} - status code: {}", uri_str, resp.status()).into());
        }

        let (parts, body) = resp.into_parts();

        let resp_index: i64 = match parts.headers.get("x-consul-index") {
            Some(consul_index) => consul_index.to_str().unwrap().parse()?,
            None => {
                warn!("Missing x-consul-index header. Setting index to 0");
                0
            }
        };

        // TODO check with consul doc which condition should lead to reset
        if resp_index < prev_index {
            warn!("Consul index querying list of services is lower than previous one. Will need to reset it to 0");
            return Ok(None);
        }

        if resp_index < 0 {
            warn!("Consul index < 0. . Will need to reset it to 0");
            return Ok(None);
        }

        let bytes = hyper::body::to_bytes(body).await?;
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();
        Ok(Some(HttpCall { index: resp_index, body: body_str }))
    }

    pub async fn get_matching_services(&mut self, prev_index: i64, tag: String) -> Result<Option<WatchedServices>, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/services?index={}", self.fqdn, prev_index);

        let response = self.http_call(services_uri, prev_index).await.unwrap();
        let resp = match response {
            Some(x) => {
                x
            },
            None => return Ok(None),
        };
        let matching_services = self.extract_matching_services(&tag, resp.body);

        Ok(Some(WatchedServices { index: resp.index, services: matching_services }))
    }

    fn extract_nodes(&mut self, body_str: String) -> Vec<ServiceNode> {
        let deserialized_body: Value = match serde_json::from_str(body_str.as_str()) {
            Err(_) => {
                warn!("Body for services in consul catalog is not json parsable: {}", body_str);
                return Vec::new();
            }
            Ok(x) => x,
        };

        let empty = Vec::new();
        let services = match deserialized_body.as_array() {
            Some(x) => x,
            None => {
                warn!("Empty list of services on the consul catalog");
                &empty
            }
        };

        let nodes = services.iter().map(|node_value| {
            let empty = Map::new();
            let node = match node_value.as_object() {
                Some(x) => x,
                None => {
                    warn!("Empty list of services on the consul catalog");
                    &empty
                }
            };
            let service_address = node.get("ServiceAddress").unwrap().as_str().unwrap().to_string();
            let service_port = node.get("ServicePort").unwrap().as_i64().unwrap();
            ServiceNode { ip: service_address, port: service_port }
        }).collect::<Vec<ServiceNode>>();

        nodes
    }

    // TODO use filter with ServiceTags https://www.consul.io/api-docs/catalog#list-nodes-for-service
    // TODO us streaming
    pub async fn list_nodes_for_service(&mut self, prev_index: i64, service: String) -> Result<Option<ServiceNodes>, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/service/{}?index={}&wait=10m", self.fqdn, service, prev_index);


        let response = self.http_call(services_uri, prev_index).await.unwrap();
        let resp = match response {
            Some(x) => {
                x
            },
            None => return Ok(None),
        };

        let service_node = self.extract_nodes(resp.body);
        Ok(Some(ServiceNodes { index: resp.index, nodes: service_node }))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::consul::ConsulClient;

    #[test]
    fn get_string_value() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1);
        assert_eq!(consul_client.get_string_value(&Value::String("test".to_string())), "test".to_string());
        assert_eq!(consul_client.get_string_value(&Value::Null), "".to_string());
    }

    #[test]
    fn is_matching_service() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1);
        assert_eq!(consul_client.is_matching_service(&"elasticsearch".to_string(),
                                                     Some(&vec![Value::String("elasticsearch".to_string()), Value::String("http".to_string())])), true);
        assert_eq!(consul_client.is_matching_service(&"elasticsearch".to_string(),
                                                     Some(&vec![Value::String("memcached".to_string()), Value::String("tcp".to_string())])), false);
        assert_eq!(consul_client.is_matching_service(&"elasticsearch".to_string(),
                                                     None), false);
    }

    #[test]
    fn extract_matching_services() {
        let mut consul_client = ConsulClient::new("localhost".to_string(), 1);

        let body_str = "{\"youfollow-yourequest-admin\":[\"netcore\",\"73c7b23f2ce611ecb9d488e9a4060640\",\"admin-handler-api\",\
        \"default\",\"http\",\"marathon\",\"marathon-start-20211014T120126Z\",\"marathon-user-svc-youfollow\"],\
        \"elasticsearch-secauditlogs-https\":[\"https\",\"elasticsearch\",\"master\",\"data\",\"cluster_name-secauditlogs\",\"version-7.7.1\",\"maintenance-elasticsearch\",\"nosql\"],\
        \"elasticsearch-shared\":[\"nosql\",\"data\",\"cluster_name-shared-s01\",\"version-6.8.10\",\"\",\"https\",\"elasticsearch\",\"master\",\"maintenance-elasticsearch\"]}".to_string();
        assert_eq!(consul_client.extract_matching_services(&"maintenance-elasticsearch".to_string(), body_str), vec!["elasticsearch-secauditlogs-https", "elasticsearch-shared"]);

        let empty: Vec<String> = Vec::new();
        // Empty json for list of services
        assert_eq!(consul_client.extract_matching_services(&"maintenance-elasticsearch".to_string(),
                                                           "{}".to_string()), empty);
        // Empty string for list of services
        assert_eq!(consul_client.extract_matching_services(&"maintenance-elasticsearch".to_string(),
                                                           "".to_string()), empty);
    }
}