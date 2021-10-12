use probes::consul::ConsulClient;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mt_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("EsPoke")
        .build()?;

    mt_rt.block_on(
        async {
            let mut consul_client = ConsulClient::new("kube-worker0005.kubes98.par.preprod.crto.in".to_string(), 8500);
            consul_client.watch_services().await;
        });
    Ok(())
}