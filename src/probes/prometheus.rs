use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use lazy_static::lazy_static;
use prometheus::{
    register_histogram_vec, register_int_counter, register_int_counter_vec, HistogramOpts,
    HistogramVec, IntCounter, IntCounterVec, Opts,
};
use tracing::{error, info};

lazy_static! {
    pub static ref NUMBER_OF_REQUESTS: IntCounterVec = register_int_counter_vec!(
        Opts::new("number_of_requests", "Number of total requests"),
        &["cluster_name", "socket", "status", "type"]
    )
    .expect("metric can be created");
    pub static ref RESPONSE_TIME_COLLECTOR: HistogramVec = register_histogram_vec!(
        HistogramOpts::new("response_time_seconds", "Response Times").buckets(vec![
            0.00001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
            2.5, 5.0, 10.0,
        ]),
        &["cluster_name", "socket", "type"]
    )
    .expect("metric can be created");
    pub static ref FAILURE_SERVICES_DISCOVERY: IntCounter = register_int_counter!(
        "failure_services_discovery",
        "Number of service discovery failed"
    )
    .expect("metric can be created");
    pub static ref FAILURE_PROBE: IntCounterVec = register_int_counter_vec!(
        Opts::new("failure_probe", "Failed to run probe action"),
        &["cluster_name", "socket"]
    )
    .expect("metric can be created");
}

/// Handler of healthz endpoint
///
/// # Return
///
/// * Return ok string
///
async fn healthz_handler() -> Result<&'static str, StatusCode> {
    Ok("ok")
}

/// Handler of metrics endpoint
///
/// transform default and custom metrics to a string
///
/// # Return
///
/// * Return prometheus metrics string or https status code representing the faced issue
///
async fn metrics_handler() -> Result<String, StatusCode> {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(_e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        //error!("could not encode prometheus metrics: {}", e.into());
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let res = match String::from_utf8(buffer) {
        Ok(v) => v,
        Err(e) => {
            error!("prometheus metrics could not be from_utf8'd: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    Ok(res)
}

/// Initialize the webserver for healthz and metrics endpoint
/// Used to expose prometheus metrics
///
/// # Arguments
///
/// * `http_port` - listening port of the webserver
///
pub async fn init_prometheus_http_endpoint(
    http_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/metrics", get(metrics_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    info!("Http server for metrics endpoint listening on {}", addr);
    match axum::Server::try_bind(&addr) {
        Ok(server) => server.serve(app.into_make_service()).await?,
        Err(issue) => return Err(issue.into()),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::probes::prometheus::NUMBER_OF_REQUESTS;
    use crate::probes::prometheus::{healthz_handler, metrics_handler};

    #[tokio::test]
    async fn test_healthz_handler() {
        assert_eq!("ok", healthz_handler().await.unwrap());
    }

    #[tokio::test]
    async fn test_metrics_handler() {
        NUMBER_OF_REQUESTS
            .with_label_values(&["cluster_name", "addr", "status_code", "get"])
            .inc();
        NUMBER_OF_REQUESTS
            .with_label_values(&["cluster_name", "addr", "status_code", "get"])
            .inc();
        NUMBER_OF_REQUESTS
            .with_label_values(&["cluster_name", "addr", "status_code", "set"])
            .inc();
        let metrics = metrics_handler().await.unwrap();
        assert!(metrics.contains("process_cpu_seconds_total"));
        assert!(metrics.contains("number_of_requests{cluster_name=\"cluster_name\",socket=\"addr\",status=\"status_code\",type=\"get\"} 2"));
        assert!(metrics.contains("number_of_requests{cluster_name=\"cluster_name\",socket=\"addr\",status=\"status_code\",type=\"set\"} 1"));
    }
}
