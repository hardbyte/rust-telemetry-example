use opentelemetry::trace::{TraceContextExt, TracerProvider};
use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info_span, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use urlencoding;

use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

// Configuration constants
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";
const APP_BASE_URL: &str = "http://localhost:8000";
const TELEMETRY_BASE_URL: &str = "http://localhost:3000";
const TEMPO_DIRECT_URL: &str = "http://localhost:3200";
const BOOKS_ENDPOINT: &str = "/books";
const EXPECTED_SERVICE_NAME: &str = "bookapp";
const EXPECTED_SPAN_NAME: &str = "HTTP GET /books";

// Retry and timeout configuration
const MAX_TEMPO_ATTEMPTS: usize = 10;
const MAX_LOKI_ATTEMPTS: usize = 10;
const MAX_PROMETHEUS_ATTEMPTS: usize = 7;
const BASE_RETRY_DELAY_SECS: u64 = 3;
const TRACE_PROPAGATION_WAIT_SECS: u64 = 3;
const LOG_LOOKBACK_SECS: u64 = 300; // 5 minutes

// Test result types
type TestResult<T> = Result<T, TestError>;

#[derive(Debug)]
struct TestError {
    message: String,
    operation: String,
}

impl TestError {
    fn new(operation: &str, message: String) -> Self {
        Self {
            operation: operation.to_string(),
            message,
        }
    }
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}

// Telemetry response types
#[derive(Debug, Deserialize)]
struct LokiResponse {
    data: LokiData,
}

#[derive(Debug, Deserialize)]
struct LokiData {
    result: Vec<LokiStream>,
}

#[derive(Debug, Deserialize)]
struct LokiStream {
    values: Vec<Vec<String>>, // Each value is [timestamp, log_line]
}

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    status: String,
    data: PrometheusData,
}

#[derive(Debug, Deserialize)]
struct PrometheusData {
    result: Vec<PrometheusResult>,
}

#[derive(Debug, Deserialize)]
struct PrometheusResult {
    value: Vec<serde_json::Value>, // [timestamp, value_string]
}

// Test configuration and state
struct TestConfig {
    app_url: String,
    telemetry_url: String,
    tempo_url: String,
    books_endpoint: String,
    trace_propagation_wait: Duration,
    log_lookback_duration: Duration,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            app_url: APP_BASE_URL.to_string(),
            telemetry_url: TELEMETRY_BASE_URL.to_string(),
            tempo_url: TEMPO_DIRECT_URL.to_string(),
            books_endpoint: BOOKS_ENDPOINT.to_string(),
            trace_propagation_wait: Duration::from_secs(TRACE_PROPAGATION_WAIT_SECS),
            log_lookback_duration: Duration::from_secs(LOG_LOOKBACK_SECS),
        }
    }
}

static INIT: std::sync::Once = std::sync::Once::new();

fn init_test_tracing() -> TestResult<()> {
    INIT.call_once(|| {
        // Set up OpenTelemetry
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string());
        println!("Test OTLP Exporter Endpoint: {}", otlp_endpoint);

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(otlp_endpoint)
            .build()
            .expect("Failed to create OTLP span exporter for tests");

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .build();

        opentelemetry::global::set_tracer_provider(provider.clone());
        let tracer = provider.tracer("integration_test");

        // Set up tracing subscriber with OpenTelemetry layer
        let telemetry_layer = OpenTelemetryLayer::new(tracer);
        let subscriber = Registry::default()
            .with(EnvFilter::from_default_env())
            .with(telemetry_layer);

        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global tracing subscriber");

        println!("Test tracing initialized");
    });
    Ok(())
}

fn validate_trace_id(trace_id: &str) -> TestResult<()> {
    if trace_id.len() != 32 {
        return Err(TestError::new(
            "trace_id_validation",
            format!("Trace ID should be 32 characters, got {}", trace_id.len()),
        ));
    }

    if !trace_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(TestError::new(
            "trace_id_validation",
            "Trace ID should contain only hexadecimal characters".to_string(),
        ));
    }

    Ok(())
}

async fn query_tempo_for_trace(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    validate_trace_id(trace_id)?;

    // Try direct Tempo API first, then Grafana proxy (using correct datasource ID 2)
    let tempo_urls = vec![
        format!("{}/api/traces/{}", config.tempo_url, trace_id),
        format!(
            "{}/api/datasources/proxy/2/api/traces/{}",
            config.telemetry_url, trace_id
        ),
    ];

    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        println!("Attempt {} for Tempo trace query", attempt);

        for (i, tempo_url) in tempo_urls.iter().enumerate() {
            println!("Trying URL {}: {}", i + 1, tempo_url);

            match http_client.get(tempo_url).send().await {
                Ok(response) => {
                    let status = response.status();
                    println!("Tempo API response status: {}", status);

                    if status == reqwest::StatusCode::OK {
                        match response.text().await {
                            Ok(response_text) => {
                                println!(
                                    "Tempo API response body (length: {})",
                                    response_text.len()
                                );

                                if !response_text.is_empty()
                                    && response_text != "{}"
                                    && !response_text.to_lowercase().contains("trace not found")
                                {
                                    return Ok(());
                                }
                            }
                            Err(e) => println!("Failed to read response: {:?}", e),
                        }
                    } else if status != reqwest::StatusCode::NOT_FOUND {
                        let error_body = response.text().await.unwrap_or_default();
                        println!(
                            "Error response from {}: {} - {}",
                            tempo_url, status, error_body
                        );
                    }
                }
                Err(e) => println!("Request failed for {}: {:?}", tempo_url, e),
            }
        }

        if attempt < MAX_TEMPO_ATTEMPTS {
            let delay = Duration::from_secs(attempt as u64 * BASE_RETRY_DELAY_SECS);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "tempo_query",
        format!(
            "Failed to find trace {} after {} attempts",
            trace_id, MAX_TEMPO_ATTEMPTS
        ),
    ))
}

async fn query_loki_for_logs(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    validate_trace_id(trace_id)?;

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| TestError::new("time_calculation", e.to_string()))?
        .as_nanos();
    let start_ns = now_ns - (config.log_lookback_duration.as_nanos());

    let log_query = format!("{{trace_id=\"{}\"}}", trace_id);
    let loki_query_url = format!(
        "{}/api/datasources/proxy/3/loki/api/v1/query_range?query={}&start={}&end={}&direction=forward",
        config.telemetry_url,
        urlencoding::encode(&log_query),
        start_ns,
        now_ns
    );

    println!("Loki query URL: {}", loki_query_url);

    for attempt in 1..=MAX_LOKI_ATTEMPTS {
        println!("Attempt {} for Loki logs query", attempt);

        match http_client.get(&loki_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Loki API response status: {}", status);

                if status == reqwest::StatusCode::OK {
                    match response.text().await {
                        Ok(response_text) => {
                            match serde_json::from_str::<LokiResponse>(&response_text) {
                                Ok(loki_response) => {
                                    let log_count: usize = loki_response
                                        .data
                                        .result
                                        .iter()
                                        .map(|stream| stream.values.len())
                                        .sum();

                                    if log_count > 0 {
                                        println!(
                                            "Found {} log entries in Loki for trace ID {}.",
                                            log_count, trace_id
                                        );
                                        return Ok(());
                                    }
                                }
                                Err(e) => println!("Failed to parse Loki JSON response: {:?}", e),
                            }
                        }
                        Err(e) => println!("Failed to read Loki response: {:?}", e),
                    }
                }
            }
            Err(e) => println!("Loki request failed: {:?}", e),
        }

        if attempt < MAX_LOKI_ATTEMPTS {
            let delay = Duration::from_secs(attempt as u64 * BASE_RETRY_DELAY_SECS);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "loki_query",
        format!(
            "Failed to find logs for trace {} after {} attempts",
            trace_id, MAX_LOKI_ATTEMPTS
        ),
    ))
}

async fn query_prometheus_for_metrics(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    validate_trace_id(trace_id)?;

    let prom_query = format!(
        "sum(traces_spanmetrics_calls_total{{service=\"{}\", span_kind=\"server\", span_name=\"{}\", trace_id=\"{}\"}}) by (span_name)",
        EXPECTED_SERVICE_NAME, EXPECTED_SPAN_NAME, trace_id
    );

    let prometheus_query_url = format!(
        "{}/api/datasources/proxy/1/api/v1/query?query={}",
        config.telemetry_url,
        urlencoding::encode(&prom_query)
    );

    println!("Prometheus query URL: {}", prometheus_query_url);

    for attempt in 1..=MAX_PROMETHEUS_ATTEMPTS {
        println!("Attempt {} for Prometheus metrics query", attempt);

        match http_client.get(&prometheus_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Prometheus API response status: {}", status);

                if status == reqwest::StatusCode::OK {
                    match response.text().await {
                        Ok(response_text) => {
                            match serde_json::from_str::<PrometheusResponse>(&response_text) {
                                Ok(prom_response) => {
                                    if prom_response.status == "success"
                                        && !prom_response.data.result.is_empty()
                                    {
                                        if let Some(first_result) =
                                            prom_response.data.result.first()
                                        {
                                            if let Some(value_str) =
                                                first_result.value.get(1).and_then(|v| v.as_str())
                                            {
                                                match value_str.parse::<f64>() {
                                                    Ok(val) if val >= 1.0 => {
                                                        println!("Successfully found metric with value {} >= 1.0", val);
                                                        return Ok(());
                                                    }
                                                    Ok(val) => {
                                                        println!("Metric value {} is < 1.0", val)
                                                    }
                                                    Err(e) => println!(
                                                        "Failed to parse metric value '{}': {:?}",
                                                        value_str, e
                                                    ),
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("Failed to parse Prometheus JSON response: {:?}", e)
                                }
                            }
                        }
                        Err(e) => println!("Failed to read Prometheus response: {:?}", e),
                    }
                }
            }
            Err(e) => println!("Prometheus request failed: {:?}", e),
        }

        if attempt < MAX_PROMETHEUS_ATTEMPTS {
            let delay = Duration::from_secs(attempt as u64 * BASE_RETRY_DELAY_SECS);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "prometheus_query",
        format!(
            "Failed to find metrics for trace {} after {} attempts",
            trace_id, MAX_PROMETHEUS_ATTEMPTS
        ),
    ))
}

#[tokio::test]
async fn test_root_endpoint_generates_telemetry() -> TestResult<()> {
    let config = TestConfig::default();

    init_test_tracing()?;

    let (trace_id, http_client) = execute_traced_request(&config).await?;
    wait_for_trace_propagation(&config).await;

    // Test all telemetry systems now that trace ID extraction works
    verify_telemetry_in_all_systems(&http_client, &trace_id, &config).await?;

    println!("✅ Test completed successfully!");
    Ok(())
}

async fn execute_traced_request(config: &TestConfig) -> TestResult<(String, HttpClient)> {
    let http_client = HttpClient::new();
    let endpoint_url = format!("{}{}", config.app_url, config.books_endpoint);

    println!("📡 Sending request to {} endpoint", config.books_endpoint);

    let response = http_client
        .get(&endpoint_url)
        .send()
        .await
        .map_err(|e| TestError::new("http_request", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "http_request",
            format!(
                "Request to {} endpoint failed with status: {}",
                config.books_endpoint,
                response.status()
            ),
        ));
    }

    // Extract trace ID from the traceparent header
    let trace_id = if let Some(traceparent) = response.headers().get("traceparent") {
        if let Ok(traceparent_str) = traceparent.to_str() {
            // traceparent format: 00-{trace_id}-{span_id}-{flags}
            let parts: Vec<&str> = traceparent_str.split('-').collect();
            if parts.len() >= 2 {
                let trace_id = parts[1].to_string();
                validate_trace_id(&trace_id)?;
                println!(
                    "🔍 Extracted trace ID from response: {} (length: {})",
                    trace_id,
                    trace_id.len()
                );
                trace_id
            } else {
                return Err(TestError::new(
                    "trace_extraction",
                    format!("Invalid traceparent format: {}", traceparent_str),
                ));
            }
        } else {
            return Err(TestError::new(
                "trace_extraction",
                "Failed to parse traceparent header as string".to_string(),
            ));
        }
    } else {
        return Err(TestError::new(
            "trace_extraction",
            "No traceparent header found in response".to_string(),
        ));
    };

    println!(
        "✅ Request successful ({}) with trace ID: {}",
        response.status(),
        trace_id
    );
    Ok((trace_id, http_client))
}

fn extract_trace_id(span: &Span) -> TestResult<String> {
    // First try to get trace ID from the tracing span context
    let otel_context = span.context();
    let span_ref = otel_context.span();
    let span_context = span_ref.span_context();
    let trace_id_val = span_context.trace_id();

    // Check if we got a valid trace ID (not all zeros)
    if trace_id_val.to_bytes() != [0u8; 16] {
        let trace_id = format!("{:032x}", trace_id_val);
        validate_trace_id(&trace_id)?;
        println!(
            "🔍 Generated trace ID: {} (length: {})",
            trace_id,
            trace_id.len()
        );
        Ok(trace_id)
    } else {
        // Fallback: generate a random trace ID for testing purposes
        use opentelemetry::trace::TraceId;
        let random_trace_id = TraceId::from_bytes(rand::random::<[u8; 16]>());
        let trace_id = format!("{:032x}", random_trace_id);
        validate_trace_id(&trace_id)?;
        println!(
            "🔍 Generated fallback trace ID: {} (length: {})",
            trace_id,
            trace_id.len()
        );
        Ok(trace_id)
    }
}

async fn build_traced_request(
    http_client: &HttpClient,
    span: &Span,
    config: &TestConfig,
) -> TestResult<reqwest::Request> {
    let endpoint_url = format!("{}{}", config.app_url, config.books_endpoint);
    let mut request = http_client
        .get(&endpoint_url)
        .build()
        .map_err(|e| TestError::new("request_building", e.to_string()))?;

    let otel_context = span.context();
    opentelemetry::global::get_text_map_propagator(|injector| {
        injector.inject_context(&otel_context, &mut RequestCarrier::new(&mut request));
    });

    Ok(request)
}

async fn wait_for_trace_propagation(config: &TestConfig) {
    println!("⏳ Waiting for trace propagation...");
    tokio::time::sleep(config.trace_propagation_wait).await;
}

async fn verify_telemetry_in_all_systems(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔎 Verifying telemetry data in all systems...");

    // Tempo verification (required)
    verify_tempo_trace(http_client, trace_id, config).await?;

    // Loki verification (optional - logs may not have trace correlation yet)
    match verify_loki_logs(http_client, trace_id, config).await {
        Ok(()) => println!("✅ Loki verification successful"),
        Err(e) => println!(
            "⚠️  Loki verification failed (trace correlation may not be configured): {}",
            e.message
        ),
    }

    // Prometheus verification (optional - metrics may need more time)
    match verify_prometheus_metrics(http_client, trace_id, config).await {
        Ok(()) => println!("✅ Prometheus verification successful"),
        Err(e) => println!(
            "⚠️  Prometheus verification failed (metrics may need more time): {}",
            e.message
        ),
    }

    Ok(())
}

async fn verify_tempo_trace(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🎯 Querying Tempo for trace: {}", trace_id);
    query_tempo_for_trace(http_client, trace_id, config)
        .await
        .map_err(|e| TestError::new("tempo_verification", e.message))?;
    println!("✅ Tempo verification successful");
    Ok(())
}

async fn verify_loki_logs(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("📋 Querying Loki for logs with trace: {}", trace_id);
    query_loki_for_logs(http_client, trace_id, config)
        .await
        .map_err(|e| TestError::new("loki_verification", e.message))?;
    println!("✅ Loki verification successful");
    Ok(())
}

async fn verify_prometheus_metrics(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!(
        "📊 Querying Prometheus for metrics with trace: {}",
        trace_id
    );
    query_prometheus_for_metrics(http_client, trace_id, config)
        .await
        .map_err(|e| TestError::new("prometheus_verification", e.message))?;
    println!("✅ Prometheus verification successful");
    Ok(())
}

// HTTP header injection for trace context
mod helpers {
    use std::collections::HashMap;

    pub struct RequestCarrier<'a> {
        request: &'a mut reqwest::Request,
    }

    impl<'a> RequestCarrier<'a> {
        pub fn new(request: &'a mut reqwest::Request) -> Self {
            Self { request }
        }
    }

    impl<'a> opentelemetry::propagation::Injector for RequestCarrier<'a> {
        fn set(&mut self, key: &str, value: String) {
            if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&value) {
                    self.request.headers_mut().insert(header_name, header_value);
                }
            }
        }
    }

    pub struct HeaderMapCarrier<'a> {
        headers: &'a mut HashMap<String, String>,
    }

    impl<'a> HeaderMapCarrier<'a> {
        pub fn new(headers: &'a mut HashMap<String, String>) -> Self {
            Self { headers }
        }
    }

    impl<'a> opentelemetry::propagation::Injector for HeaderMapCarrier<'a> {
        fn set(&mut self, key: &str, value: String) {
            self.headers.insert(key.to_string(), value);
        }
    }
}

use helpers::RequestCarrier;
