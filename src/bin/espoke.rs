use hyper::{Body, Client};
use hyper::client::HttpConnector;

use probes::token_bucket::TokenBucket;

async fn get_services(client: &Client<HttpConnector, Body>, prev_index: i64) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
    let fqdn = format!("http://kube-worker0005.kubes98.par.preprod.crto.in:8500/v1/catalog/services?index={}", prev_index);

    let uri = fqdn.as_str().parse()?;

    let resp = client.get(uri).await?;

    if !resp.status().is_success() {
        return Err(format!("Issue query: {} - status code: {}", fqdn, resp.status()).into());
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

async fn watch_services() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let mut index = 0;

    let mut token_bucket = TokenBucket::new(60, 1);

    loop {
        token_bucket.wait_for(60).await?;

        index = get_services(&client, index).await?;
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Still inside `async fn main`...
    watch_services().await?;

    Ok(())
}