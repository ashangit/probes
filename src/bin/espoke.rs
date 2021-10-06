use hyper::{Client, Body, Response};

use hyper::body::HttpBody as _;
use tokio::io::{stdout, AsyncWriteExt as _};
use hyper::header::HeaderValue;
use hyper::client::HttpConnector;
use std::fmt::{Display, Formatter};

async fn get_services(client: &Client<HttpConnector, Body>, index: Option<&HeaderValue>) -> Result<Response<Body>, Box<dyn std::error::Error + Send + Sync>> {
    let prev_index: i64 = match index {
        Some(x) => {
            let tmp_index = x.to_str().unwrap().parse()?;
            if tmp_index < 0 {
                0
            } else {
                tmp_index
            }
        }
        None => 0,
    };

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

    // TODO: change the return type so it return index and resp
    if resp_index < prev_index {
        println!("ISSUE");
    }

    // TODO: rate limit request: https://docs.rs/tokenbucket/0.1.3/tokenbucket/ + https://docs.rs/tokio/1.0.1/tokio/time/fn.sleep.html

    Ok(resp)
}

async fn watch_services() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = Client::new();
    let mut index = None;

    //async move {
        while true {
            let resp = get_services(&client, index).await.unwrap();
            index = resp.headers().get("x-consul-index");
            println!("{}", index.unwrap().to_str().unwrap())

            // And now...
            //while let Some(chunk) = resp.body_mut().data().await {
            //    stdout().write_all(&chunk?).await?;
            //}
        };
   // };
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Still inside `async fn main`...
    watch_services().await?;

    Ok(())
}