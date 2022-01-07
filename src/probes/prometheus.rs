use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use lazy_static::lazy_static;
use prometheus::{HistogramOpts, HistogramVec, IntCounter, IntCounterVec, Opts, Registry};
use tracing::{error, info};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref NUMBER_OF_REQUESTS: IntCounterVec = IntCounterVec::new(
        Opts::new("number_of_requests", "Number of total requests"),
        &["cluster_name", "socket", "status", "type"]
    )
    .expect("metric can be created");
    pub static ref RESPONSE_TIME_COLLECTOR: HistogramVec = HistogramVec::new(
        HistogramOpts::new("response_time_seconds", "Response Times").buckets(vec![
            0.00001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0,
            2.5, 5.0, 10.0,
        ]),
        &["cluster_name", "socket", "type"]
    )
    .expect("metric can be created");
    pub static ref FAILURE_SERVICES_DISCOVERY: IntCounter = IntCounter::new(
        "failure_services_discovery",
        "Number of service discovery failed"
    )
    .expect("metric can be created");
    pub static ref FAILURE_PROBE: IntCounterVec = IntCounterVec::new(
        Opts::new("failure_probe", "Failed to run probe action"),
        &["cluster_name", "socket"]
    )
    .expect("metric can be created");
}

/// Register custom prometheus metrics in the custom prometheus registry
pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(NUMBER_OF_REQUESTS.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(RESPONSE_TIME_COLLECTOR.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(FAILURE_SERVICES_DISCOVERY.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(FAILURE_PROBE.clone()))
        .expect("collector can be registered");
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
    if let Err(_e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        //error!("could not encode custom metrics: {}", e.into());
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut res = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("custom metrics could not be from_utf8'd: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    buffer.clear();

    let mut buffer = Vec::new();
    if let Err(_e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        //error!("could not encode prometheus metrics: {}", e.into());
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let res_custom = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("prometheus metrics could not be from_utf8'd: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };
    buffer.clear();

    res.push_str(&res_custom);
    Ok(res)
}

/// Initialize the webserver for healthz and metrics endpoint
/// Used to expose prometheus metrics
///
/// # Arguments
///
/// * `http_port` - listening port of the webserver
///
pub async fn init_prometheus_http_endpoint(http_port: u16) {
    let app = Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/metrics", get(metrics_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    info!("Http server for metrics endpoint listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
