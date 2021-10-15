use probes::consul::ConsulClient;
use console_subscriber;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();
    console_subscriber::init();

    // TODO: Create dedicated worker pool for probing and use mono thread runtime for services discovery?
    // TODO check all unwrap and improve exception management
    // TODO command parser
    let mt_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("EsPoke")
        .build()?;

    mt_rt.block_on(
        async {
            let mut consul_client = ConsulClient::new("kube-worker0005.kubes98.par.preprod.crto.in".to_string(), 8500, "maintenance-elasticsearch".to_string());
            consul_client.watch_services().await;
        });
    Ok(())
}