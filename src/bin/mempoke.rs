use argparse::{ArgumentParser, Store};
use log::error;

use probes::probes::init_probing;
use probes::probes::prometheus::{init_prometheus_http_endpoint, register_custom_metrics};

fn main() -> Result<(), i32> {
    env_logger::init();
    register_custom_metrics();

    let mut consul_fqdn = "http://localhost:8500".to_string();
    let mut http_port = 8080;
    let mut services_tag = "".to_string();
    let mut tokio_console = false;

    {
        // this block limits scope of borrows by ap.refer() method
        let mut argument_parser = ArgumentParser::new();
        argument_parser.set_description("Memcached Probe (MemPoke)");
        argument_parser.refer(&mut consul_fqdn).add_option(
            &["--consul-fqdn"],
            Store,
            "Consul hostname (default: http://localhost:8500)",
        );
        argument_parser
            .refer(&mut services_tag)
            .add_option(
                &["--services-tag"],
                Store,
                "Tag to select services to probe",
            )
            .required();
        argument_parser.refer(&mut tokio_console).add_option(
            &["--tokio-console"],
            Store,
            "Enable console subscriber for the tokio console (default: false)",
        );
        argument_parser.refer(&mut http_port).add_option(
            &["--http-port"],
            Store,
            "Http port for metrics endpoint (default: 8080)",
        );
        argument_parser.parse_args_or_exit();
    }

    // Init tokio console subscriber if enabled
    // Used to debug trace async task with https://github.com/tokio-rs/console
    if tokio_console {
        console_subscriber::init();
    }

    // Init multi thread tokio scheduler
    let multi_thread_runtime_res = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("MemPoke")
        .build();

    match multi_thread_runtime_res {
        Ok(multi_thread_runtime) => {
            // Init prometheus http endpoint
            multi_thread_runtime.spawn(init_prometheus_http_endpoint(http_port));

            // Init probing
            if let Err(issue) =
                multi_thread_runtime.block_on(init_probing(services_tag, consul_fqdn))
            {
                error!("Issue during node probing: {}", issue);
                return Err(2);
            }
        }
        Err(issue) => {
            error!(
                "Issue starting multi-threaded tokio scheduler due to: {}",
                issue
            );
            return Err(1);
        }
    };

    Ok(())
}
