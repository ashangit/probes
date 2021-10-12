use hyper::{Body, Client};
use hyper::client::HttpConnector;

use crate::token_bucket::TokenBucket;

// Represent a consul client
pub struct ConsulClient {
    // The fqdn of the consul agent to query
    fqdn: String,
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
        ConsulClient {
            fqdn: format!("{}:{}", hostname, port)
        }
    }

    async fn get_services(&mut self, client: &Client<HttpConnector, Body>, prev_index: i64) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
        let services_uri = format!("http://{}/v1/catalog/services?index={}", self.fqdn, prev_index);

        let resp = client.get(services_uri.as_str().parse()?).await?;

        if !resp.status().is_success() {
            return Err(format!("Issue query: {} - status code: {}", services_uri, resp.status()).into());
        }

        println!("Response: {}", resp.status());

        for (key, value) in resp.headers().iter() {
            println!("{:?}: {:?}", key, value);
        }

        let resp_index: i64 = resp.headers().get("x-consul-index").unwrap().to_str().unwrap().parse()?;

        // TODO check with consul doc which condition should lead to reset
        if resp_index < prev_index {
            return Ok(0);
        }

        // TODO also return body

        Ok(resp_index)
    }

    pub async fn watch_services(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = Client::new();
        let mut index = 0;

        let mut token_bucket = TokenBucket::new(60, 1);

        loop {
            token_bucket.wait_for(60).await?;

            index = self.get_services(&client, index).await?;
            println!("{}", index);

            if index < 0 {
                break;
            }

            // And now...
            //while let Some(chunk) = resp.body_mut().data().await {
            //    stdout().write_all(&chunk?).await?;
            //}
        };
        Ok(())
    }
}
