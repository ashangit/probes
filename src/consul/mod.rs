use std::collections::HashMap;
use std::fmt;

use hyper::client::HttpConnector;
use hyper::{Client, Uri};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use serde_json::{Map, Value};
use tracing::log::warn;
use tracing::{debug, error};

// Represent a consul client
#[derive(Debug, Clone)]
pub struct ConsulClient {
    // The fqdn of the consul agent to query
    fqdn: String,
    client: Client<HttpsConnector<HttpConnector>>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ServiceNode {
    pub service_name: String,
    pub ip: String,
    pub port: u16,
}

impl fmt::Display for ServiceNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.service_name, self.ip, self.port)
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct ServiceNodes {
    pub index: i64,
    pub nodes: HashMap<String, ServiceNode>,
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
    /// let mut consul_client = ConsulClient::new("http://localhost:8500".to_string());
    /// ```
    pub fn new(consul_fqdn: String) -> Self {
        debug!("Create consul client {}", consul_fqdn);
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .https_or_http()
            .enable_http1()
            .build();

        ConsulClient {
            fqdn: consul_fqdn,
            client: Client::builder().build::<_, hyper::Body>(https),
        }
    }

    /// Get string from json value
    ///
    /// # Arguments
    ///
    /// * `value` - json value
    ///
    /// # Return
    ///
    /// * String - the String of the Value or an empty String if the value doesn't contains a String
    ///
    fn get_string_value(value: &Value) -> String {
        if let Some(x) = value.as_str() {
            x.to_string()
        } else {
            "".to_string()
        }
    }

    /// Check if tag for probing is available in the list of service tags
    ///
    /// # Arguments
    ///
    /// * `tag` - tag needed on service to enable probing
    /// * `tags_opt` - list of tags set on the service
    ///
    /// # Return
    ///
    /// * bool - true if the list of tags_opt contains tag
    ///
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

    /// Extract list of services with tag for probing
    ///
    /// # Arguments
    ///
    /// * `tag` - tag needed on service to enable probing
    /// * `body_json` - json from consul catalog services
    ///
    /// # Return
    ///
    /// * List String - the list of matching service
    ///
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

    /// Create ServiceNode from json representing a node in consul service
    ///
    /// # Arguments
    ///
    /// * `service_name` - name of the service in consul
    /// * `node_value` - json representing a node in consul service
    ///
    /// # Return
    ///
    /// * ServiceNode - the definition of a node to probe with service_name, ip and port
    ///
    fn get_service_address_port(service_name: &str, node_value: &Value) -> ServiceNode {
        let node = node_value.as_object().unwrap();
        let service_address = node
            .get("ServiceAddress")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let service_port: u16 = node.get("ServicePort").unwrap().as_u64().unwrap() as u16;

        ServiceNode {
            service_name: service_name.to_owned(),
            ip: service_address,
            port: service_port,
        }
    }

    /// Extract list of ServiceNodes from consul service json of a specific service
    ///
    /// # Arguments
    ///
    /// * `service_name` - name of the service in consul
    /// * `body_json` - json from consul service of a specific service
    ///
    /// # Return
    ///
    /// * List ServiceNode - the list of node to probe for a specific service
    ///
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
            .map(|val| ConsulClient::get_service_address_port(&service_name, val))
            .collect::<Vec<ServiceNode>>();

        nodes
    }

    /// Get watch index value
    ///
    /// The index returned by consul for watch must be greater than 0 and grater than previous index
    /// otherwise it should be reset to 0
    ///
    /// # Arguments
    ///
    /// * `prev_index` - previous index value
    /// * `_index` - new index value
    ///
    /// # Return
    ///
    /// * int - index value that will be used on next watch
    ///
    fn get_watch_index(prev_index: i64, _index: i64) -> i64 {
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

    /// Manage http call to consul agent endpoint
    ///
    /// Rely on watch mechanism based on index value
    ///
    /// # Arguments
    ///
    /// * `uri_str` - consul uri to call
    /// * `prev_index` - index value of last http call
    ///
    /// # Return
    ///
    /// * Result of HttpCall or Error - HttpCall contains the new index for watching
    ///   and the return json body from consul
    ///
    async fn http_call(
        &mut self,
        uri_str: String,
        prev_index: i64,
    ) -> Result<HttpCall, Box<dyn std::error::Error + Send + Sync>> {
        let query_uri = format!("{}?index={}&wait=5m", uri_str, prev_index);
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
            ConsulClient::get_watch_index(prev_index, _index)
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

    /// Get the list of nodes for a service from consul endpoint
    ///
    /// # Arguments
    ///
    /// * `service_name` - name of the consul service
    ///
    /// # Return
    ///
    /// * Result of List ServiceNode or Error
    ///
    async fn list_nodes_for_service(
        &mut self,
        service_name: String,
    ) -> Result<Vec<ServiceNode>, Box<dyn std::error::Error + Send + Sync>> {
        let service_uri = format!("{}/v1/catalog/service/{}", self.fqdn, service_name);

        let response = self.http_call(service_uri, 0).await?;

        let service_node = ConsulClient::extract_nodes(service_name, response.body_json);
        Ok(service_node)
    }

    /// Get the list of nodes for all services with tags matching the tag for probing
    ///
    /// # Arguments
    ///
    /// * `prev_index` - index value of last consul watch
    /// * `tag` - tag needed on service to enable probing
    ///
    /// # Return
    ///
    /// * Result of List ServiceNode or Error
    ///
    pub async fn list_matching_nodes(
        &mut self,
        prev_index: i64,
        tag: &str,
    ) -> Result<ServiceNodes, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("{}/v1/catalog/services", self.fqdn);

        let response = self.http_call(services_uri, prev_index).await?;

        let matching_services = ConsulClient::extract_matching_services(tag, response.body_json);

        let mut services_nodes: HashMap<String, ServiceNode> = HashMap::new();
        for matching_service in matching_services {
            match self.list_nodes_for_service(matching_service).await {
                Ok(service_nodes) => {
                    for service_node in service_nodes {
                        services_nodes.insert(service_node.to_string(), service_node);
                    }
                }
                Err(issue) => return Err(issue),
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
    use std::collections::HashMap;

    use serde_json::Value;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::consul::{ConsulClient, ServiceNode, ServiceNodes};

    #[test]
    fn service_node_to_string() {
        let node = ServiceNode {
            service_name: "service_name".to_string(),
            ip: "0.0.0.0".to_string(),
            port: 12500,
        };
        assert_eq!("service_name:0.0.0.0:12500".to_string(), node.to_string());
    }

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
                Value::String("http".to_string()),
            ]),
        ));
        assert!(!ConsulClient::is_matching_service(
            &"elasticsearch".to_string(),
            Some(&vec![
                Value::String("memcached".to_string()),
                Value::String("tcp".to_string()),
            ]),
        ));
        assert!(!ConsulClient::is_matching_service(
            &"elasticsearch".to_string(),
            None,
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
                body_json,
            )
        );

        let empty: Vec<String> = Vec::new();
        // Empty json for list of services
        assert_eq!(
            empty,
            ConsulClient::extract_matching_services(
                &"maintenance-elasticsearch".to_string(),
                serde_json::from_str("{}").unwrap(),
            )
        );
    }

    #[test]
    fn get_watch_index() {
        assert_eq!(5, ConsulClient::get_watch_index(1, 5));
        assert_eq!(0, ConsulClient::get_watch_index(5, 1));
        assert_eq!(0, ConsulClient::get_watch_index(1, -5));
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
                port: 1045,
            },
            ConsulClient::get_service_address_port("service_test", &node_value)
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

    async fn init_consul_client() -> ConsulClient {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/v1/catalog/services"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(
                        "{\"other\":[\"net\",\"4578\"], \"memcached-1\":[\"memcached\",\"4578\"]}",
                    )
                    .insert_header("x-consul-index", "110"),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/catalog/service/memcached-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    "[{\"ServiceAddress\":\"1.2.2.15\",\"ServicePort\":11213},{\"ServiceAddress\":\"1.2.2.16\",\"ServicePort\":11213}]",
                ),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/v1/catalog/service/service_name_non_parsable_json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(
                    "[{\"ServiceAddress\":\"1.2.2.15\",\"ServicePort\":11213},{\"ServiceAddress\":\"1.2.2.16\",\"ServicePort\":11213]",
                ),
            )
            .mount(&mock_server)
            .await;

        ConsulClient::new((&mock_server.uri()).to_string())
    }

    #[tokio::test]
    async fn list_nodes_for_service() {
        let mut consul_client = init_consul_client().await;

        let res = consul_client
            .list_nodes_for_service("memcached-1".to_string())
            .await
            .unwrap();

        assert_eq!(
            vec![
                ServiceNode {
                    service_name: "memcached-1".to_string(),
                    ip: "1.2.2.15".to_string(),
                    port: 11213
                },
                ServiceNode {
                    service_name: "memcached-1".to_string(),
                    ip: "1.2.2.16".to_string(),
                    port: 11213
                }
            ],
            res
        );

        let res = consul_client
            .list_nodes_for_service("service_name_non_parsable_json".to_string())
            .await
            .unwrap();

        let res_vec: Vec<ServiceNode> = Vec::new();
        assert_eq!(res_vec, res);
    }

    #[tokio::test]
    async fn list_matching_nodes() {
        let mut consul_client = init_consul_client().await;
        let res = consul_client
            .list_matching_nodes(1, "memcached")
            .await
            .unwrap();

        let nodes = HashMap::from([
            (
                "memcached-1:1.2.2.15:11213".to_string(),
                ServiceNode {
                    service_name: "memcached-1".to_string(),
                    ip: "1.2.2.15".to_string(),
                    port: 11213,
                },
            ),
            (
                "memcached-1:1.2.2.16:11213".to_string(),
                ServiceNode {
                    service_name: "memcached-1".to_string(),
                    ip: "1.2.2.16".to_string(),
                    port: 11213,
                },
            ),
        ]);
        assert_eq!(ServiceNodes { index: 110, nodes }, res);
    }
}
