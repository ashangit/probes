use probes::consul::ConsulClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut consul_client = ConsulClient::new("kube-worker0005.kubes98.par.preprod.crto.in".to_string(), 8500);
    consul_client.watch_services().await?;

    Ok(())
}