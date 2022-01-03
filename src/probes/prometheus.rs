use lazy_static::lazy_static;
use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref NUMBER_OF_REQUESTS: IntCounterVec = IntCounterVec::new(
        Opts::new("number_of_requests", "Number of total requests"),
        &["cluster_name", "socket", "status", "type"]
    )
    .expect("metric can be created");
    pub static ref RESPONSE_TIME_COLLECTOR: HistogramVec = HistogramVec::new(
        HistogramOpts::new("response_time_seconds", "Response Times"),
        &["cluster_name", "socket", "type"]
    )
    .expect("metric can be created");
}

pub fn register_custom_metrics() {
    REGISTRY
        .register(Box::new(NUMBER_OF_REQUESTS.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(RESPONSE_TIME_COLLECTOR.clone()))
        .expect("collector can be registered");
}

pub async fn metrics_handler() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();

    let mut buffer = Vec::new();
    if let Err(_e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        //eprintln!("could not encode custom metrics: {}", e);
    };
    let mut res = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("custom metrics could not be from_utf8'd: {}", e);
            String::default()
        }
    };
    buffer.clear();

    let mut buffer = Vec::new();
    if let Err(_e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        //eprintln!("could not encode prometheus metrics: {}", e);
    };
    let res_custom = match String::from_utf8(buffer.clone()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("prometheus metrics could not be from_utf8'd: {}", e);
            String::default()
        }
    };
    buffer.clear();

    res.push_str(&res_custom);
    res
}
