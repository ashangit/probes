use hyper::client::HttpConnector;
use hyper::{Client, Uri};
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

#[derive(Debug, PartialEq)]
pub struct ServiceNode {
    pub service_name: String,
    pub ip: String,
    pub port: i64,
}

pub struct ServiceNodes {
    pub index: i64,
    pub nodes: Vec<ServiceNode>,
}

struct HttpCall {
    index: i64,
    body_json: Value,
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
    pub fn new(hostname: String, port: u16) -> ConsulClient {
        debug!("Create consul client {}:{}", hostname, port);
        ConsulClient {
            fqdn: format!("{}:{}", hostname, port),
            client: Client::new(),
        }
    }

    fn get_string_value(value: &Value) -> String {
        if let Some(x) = value.as_str() {
            x.to_string()
        } else {
            "".to_string()
        }
    }

    fn is_matching_service(tag: &str, tags_opt: Option<&Vec<Value>>) -> bool {
        if let Some(tags) = tags_opt {
            if tags
                .iter()
                .map(ConsulClient::get_string_value)
                .any(|x| x == *tag)
            {
                return true;
            }
        }

        false
    }

    fn extract_matching_services(tag: &str, body_json: Value) -> Vec<String> {
        let empty = Map::new();
        let services = match body_json.as_object() {
            Some(x) => x,
            None => {
                warn!("Empty list of services on the consul catalog");
                &empty
            }
        };

        let matching_services = services
            .keys()
            .filter(|&key| ConsulClient::is_matching_service(tag, body_json[key].as_array()))
            .cloned()
            .collect::<Vec<String>>();

        debug!(
            "Services matching tag {}: {}",
            tag,
            matching_services.join(", ")
        );
        matching_services
    }

    fn get_service_address_port(service_name: String, node_value: &Value) -> ServiceNode {
        let node = node_value.as_object().unwrap();
        let service_address = node
            .get("ServiceAddress")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let service_port = node.get("ServicePort").unwrap().as_i64().unwrap();
        ServiceNode {
            service_name,
            ip: service_address,
            port: service_port,
        }
    }

    fn extract_nodes(service_name: String, body_json: Value) -> Vec<ServiceNode> {
        let empty = Vec::new();
        let services = match body_json.as_array() {
            Some(x) => x,
            None => {
                warn!(
                    "Returned body is not an array of nodes: {}",
                    body_json.to_string()
                );
                &empty
            }
        };

        let nodes = services
            .iter()
            .map(|val| ConsulClient::get_service_address_port(service_name.clone(), val))
            .collect::<Vec<ServiceNode>>();

        nodes
    }

    fn check_index(prev_index: i64, _index: i64) -> i64 {
        if _index < prev_index {
            warn!("Consul index querying list of services is lower than previous one. Will need to reset it to 0");
            return 0;
        }

        if _index < 0 {
            warn!("Consul index < 0. . Will need to reset it to 0");
            return 0;
        }
        _index
    }

    async fn http_call(
        &mut self,
        uri_str: String,
        prev_index: i64,
    ) -> Result<HttpCall, Box<dyn std::error::Error + Send + Sync>> {
        let query_uri = format!("{}?index={}&wait=10m", uri_str, prev_index);
        debug!("Query consul: {}", query_uri);
        let uri = match query_uri.as_str().parse::<Uri>() {
            Err(issue) => {
                error!("Invalid uri {} -> {}", query_uri, issue.to_string());
                return Err(issue.into());
            }
            Ok(_uri) => _uri,
        };

        let resp = self.client.get(uri).await?;

        if !resp.status().is_success() {
            error!("Failed to query consul, http status code {}", resp.status());
            return Err(format!(
                "Issue query: {} - status code: {}",
                query_uri,
                resp.status()
            )
            .into());
        }

        let (parts, body) = resp.into_parts();

        let resp_index: i64 = if let Some(consul_index) = parts.headers.get("x-consul-index") {
            let mut _index = consul_index.to_str().unwrap().parse()?;
            ConsulClient::check_index(prev_index, _index)
        } else {
            warn!("Missing x-consul-index header. Setting index to 0");
            0
        };

        let bytes = hyper::body::to_bytes(body).await?;
        let body_str = String::from_utf8(bytes.to_vec()).unwrap();

        let body_json: Value = match serde_json::from_str(body_str.as_str()) {
            Err(_) => {
                warn!("Http response body is not json parsable: {}", body_str);
                Value::Null
            }
            Ok(x) => x,
        };

        Ok(HttpCall {
            index: resp_index,
            body_json,
        })
    }

    async fn list_nodes_for_service(
        &mut self,
        service_name: String,
    ) -> Result<Vec<ServiceNode>, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/service/{}", self.fqdn, service_name);

        let response = self.http_call(services_uri, 0).await?;

        let service_node = ConsulClient::extract_nodes(service_name, response.body_json);
        Ok(service_node)
    }

    pub async fn list_matching_nodes(
        &mut self,
        prev_index: i64,
        tag: String,
    ) -> Result<ServiceNodes, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/services", self.fqdn);

        let response = self.http_call(services_uri, prev_index).await?;

        let matching_services = ConsulClient::extract_matching_services(&tag, response.body_json);

        let mut services_nodes: Vec<ServiceNode> = Vec::new();
        for matching_service in matching_services {
            match self.list_nodes_for_service(matching_service).await {
                Ok(mut service_nodes) => {
                    services_nodes.append(&mut service_nodes);
                }
                Err(err) => {
                    error!("Failed to get list of matching services: {}", err);
                }
            }
        }

        Ok(ServiceNodes {
            index: response.index,
            nodes: services_nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::consul::{ConsulClient, ServiceNode};

    #[test]
    fn get_string_value() {
        assert_eq!(
            "test".to_string(),
            ConsulClient::get_string_value(&Value::String("test".to_string()))
        );
        assert_eq!("".to_string(), ConsulClient::get_string_value(&Value::Null));
    }

    #[test]
    fn is_matching_service() {
        assert!(ConsulClient::is_matching_service(
            &"elasticsearch".to_string(),
            Some(&vec![
                Value::String("elasticsearch".to_string()),
                Value::String("http".to_string())
            ])
        ));
        assert!(!ConsulClient::is_matching_service(
            &"elasticsearch".to_string(),
            Some(&vec![
                Value::String("memcached".to_string()),
                Value::String("tcp".to_string())
            ])
        ));
        assert!(!ConsulClient::is_matching_service(
            &"elasticsearch".to_string(),
            None
        ));
    }

    #[test]
    fn extract_matching_services() {
        let body_json = serde_json::from_str("{\"youfollow-yourequest-admin\":[\"netcore\",\"73c7b23f2ce611ecb9d488e9a4060640\",\"admin-handler-api\",\
        \"default\",\"http\",\"marathon\",\"marathon-start-20211014T120126Z\",\"marathon-user-svc-youfollow\"],\
        \"elasticsearch-secauditlogs-https\":[\"https\",\"elasticsearch\",\"master\",\"data\",\"cluster_name-secauditlogs\",\"version-7.7.1\",\"maintenance-elasticsearch\",\"nosql\"],\
        \"elasticsearch-shared\":[\"nosql\",\"data\",\"cluster_name-shared-s01\",\"version-6.8.10\",\"\",\"https\",\"elasticsearch\",\"master\",\"maintenance-elasticsearch\"]}").unwrap();
        assert_eq!(
            vec!["elasticsearch-secauditlogs-https", "elasticsearch-shared"],
            ConsulClient::extract_matching_services(
                &"maintenance-elasticsearch".to_string(),
                body_json
            )
        );

        let empty: Vec<String> = Vec::new();
        // Empty json for list of services
        assert_eq!(
            empty,
            ConsulClient::extract_matching_services(
                &"maintenance-elasticsearch".to_string(),
                serde_json::from_str("{}").unwrap()
            )
        );
    }

    #[test]
    fn check_index() {
        assert_eq!(5, ConsulClient::check_index(1, 5));
        assert_eq!(0, ConsulClient::check_index(5, 1));
        assert_eq!(0, ConsulClient::check_index(1, -5));
    }

    #[test]
    fn get_service_address_port() {
        let node_value =
            serde_json::from_str("{\"ServiceAddress\":\"127.0.0.1\",\"ServicePort\":1045}")
                .unwrap();
        assert_eq!(
            ServiceNode {
                service_name: "service_test".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 1045
            },
            ConsulClient::get_service_address_port("service_test".to_string(), &node_value)
        );
    }

    #[test]
    fn extract_nodes() {
        let nodes_value = serde_json::from_str("[{\"ServiceAddress\":\"127.0.0.1\",\"ServicePort\":1045}, {\"ServiceAddress\":\"127.0.0.2\",\"ServicePort\":1045}]").unwrap();
        let nodes = vec![
            ServiceNode {
                service_name: "service_test".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 1045,
            },
            ServiceNode {
                service_name: "service_test".to_string(),
                ip: "127.0.0.2".to_string(),
                port: 1045,
            },
        ];
        assert_eq!(
            nodes,
            ConsulClient::extract_nodes("service_test".to_string(), nodes_value)
        );

        let nodes_value = serde_json::from_str("[]").unwrap();
        let empty: Vec<ServiceNode> = Vec::new();
        assert_eq!(
            empty,
            ConsulClient::extract_nodes("service_test".to_string(), nodes_value)
        );

        let nodes_value = serde_json::from_str("{}").unwrap();
        assert_eq!(
            empty,
            ConsulClient::extract_nodes("service_test".to_string(), nodes_value)
        );
    }
}
