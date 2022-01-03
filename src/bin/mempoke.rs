use argparse::{ArgumentParser, Store};
//use console_subscriber;
use log::error;

use warp::Filter;

use probes::consul::ConsulClient;
use probes::probes::prometheus::{metrics_handler, register_custom_metrics};
use probes::probes::ProbeServices;

fn main() -> Result<(), i32> {
    env_logger::init();
    //console_subscriber::init();
    register_custom_metrics();

    let mut consul_hostanme = "localhost".to_string();
    let mut consul_port = 8500;
    let mut http_port = 8080;
    let mut services_tag = "".to_string();
    {
        // this block limits scope of borrows by ap.refer() method
        let mut ap = ArgumentParser::new();
        ap.set_description("Memcached Probe (Mempoke)");
        ap.refer(&mut consul_hostanme).add_option(
            &["--consul-hostname"],
            Store,
            "Consul hostname (default: localhost)",
        );
        ap.refer(&mut consul_port).add_option(
            &["--consul-port"],
            Store,
            "Consul port (default: 8500)",
        );
        ap.refer(&mut services_tag)
            .add_option(
                &["--services-tag"],
                Store,
                "Tag to select services to probe",
            )
            .required();
        ap.refer(&mut http_port).add_option(
            &["--http-port"],
            Store,
            "Http port for metrics endpoint (default: 8080)",
        );
        ap.parse_args_or_exit();
    }

    // TODO: Create dedicated worker pool for probing and use mono thread runtime for services discovery?
    // TODO check all unwrap and improve exception management
    let mt_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("MemPoke")
        .build();

    match mt_rt {
        Err(issue) => {
            error!(
                "Issue running the multi thread event loop due to: {}",
                issue
            );
            return Err(0);
        }
        Ok(mt) => mt.block_on(async {
            tokio::spawn(async move {
                let metrics_route = warp::path!("metrics").and_then(metrics_handler);

                println!(
                    "Http server for metrics endpoint started on port {}",
                    http_port
                );
                warp::serve(metrics_route)
                    .run(([0, 0, 0, 0], http_port))
                    .await;
            });

            let consul_client = ConsulClient::new(consul_hostanme, consul_port);
            let mut probe = ProbeServices::new(consul_client, services_tag);
            probe.watch_matching_services().await;
        }),
    }

    Ok(())
}
