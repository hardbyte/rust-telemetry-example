use axum::{
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use opentelemetry::{metrics::Histogram, KeyValue};
use std::time::Instant;

pub async fn record_duration(
    State(duration): State<Histogram<f64>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".into());
    let started = Instant::now();
    let response = next.run(request).await;
    duration.record(
        started.elapsed().as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.route", route),
            KeyValue::new(
                "http.response.status_code",
                i64::from(response.status().as_u16()),
            ),
        ],
    );
    response
}
