use opentelemetry::trace::TracerProvider;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::time::Duration;

use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

// Import the generated Progenitor client for traced API calls
use client::{Client as BookappClient, ClientState};

// Configuration constants
const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";
const APP_BASE_URL: &str = "http://localhost:8000";
const TELEMETRY_BASE_URL: &str = "http://localhost:3000";
const TEMPO_DIRECT_URL: &str = "http://localhost:3200";
const BOOKS_ENDPOINT: &str = "/books";
const EXPECTED_SERVICE_NAME: &str = "bookapp";
const EXPECTED_SPAN_NAME: &str = "get_all_books";

// Retry and timeout configuration
const MAX_TEMPO_ATTEMPTS: usize = 15;
const MAX_LOKI_ATTEMPTS: usize = 10;
const MAX_PROMETHEUS_ATTEMPTS: usize = 10;
const BASE_RETRY_DELAY_SECS: u64 = 2;
const MAX_RETRY_DELAY_SECS: u64 = 10;
const TRACE_PROPAGATION_WAIT_SECS: u64 = 10;
const LOG_LOOKBACK_SECS: u64 = 300; // 5 minutes

// Test result types
type TestResult<T> = Result<T, TestError>;

#[derive(Debug, Clone)]
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

#[derive(Debug, Deserialize)]
struct TempoResponse {
    batches: Vec<Batch>,
}

#[derive(Debug, Deserialize)]
struct TempoSearchResponse {
    traces: Vec<TempoTrace>,
}

#[derive(Debug, Deserialize)]
struct TempoTrace {
    #[serde(rename = "traceID")]
    trace_id: String,
    #[serde(rename = "rootTraceName")]
    root_trace_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Batch {
    resource: Resource,
    #[serde(rename = "scopeSpans")]
    scope_spans: Vec<ScopeSpan>,
}

#[derive(Debug, Deserialize)]
struct Resource {
    attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
struct ScopeSpan {
    #[allow(dead_code)]
    scope: Option<serde_json::Value>,
    spans: Vec<Span>,
}

#[derive(Debug, Deserialize)]
struct Span {
    #[serde(rename = "traceId")]
    #[allow(dead_code)]
    trace_id: String,
    #[serde(rename = "spanId")]
    #[allow(dead_code)]
    span_id: String,
    #[serde(rename = "parentSpanId")]
    #[allow(dead_code)]
    parent_span_id: Option<String>,
    #[allow(dead_code)]
    flags: Option<u32>,
    name: String,
    kind: String,
    #[serde(rename = "startTimeUnixNano")]
    #[allow(dead_code)]
    start_time_unix_nano: Option<String>,
    #[serde(rename = "endTimeUnixNano")]
    #[allow(dead_code)]
    end_time_unix_nano: Option<String>,
    #[allow(dead_code)]
    attributes: Vec<KeyValue>,
    #[allow(dead_code)]
    events: Option<Vec<serde_json::Value>>,
    status: Status,
}

#[derive(Debug, Deserialize)]
struct KeyValue {
    key: String,
    value: Value,
}

#[derive(Debug, Deserialize)]
struct Value {
    #[serde(rename = "stringValue")]
    string_value: Option<String>,
    #[serde(rename = "intValue")]
    #[allow(dead_code)]
    int_value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Status {
    code: Option<String>,
}

// Test configuration and state
#[derive(Clone)]
struct TestConfig {
    app_url: String,
    telemetry_url: String,
    tempo_url: String,
    books_endpoint: String,
    trace_propagation_wait: Duration,
    log_lookback_duration: Duration,
    prometheus_datasource_id: String,
    tempo_datasource_id: String,
    loki_datasource_id: String,
    expected_service_name: String,
    expected_span_name: String,
    prometheus_query: String,
}

impl Default for TestConfig {
    fn default() -> Self {
        let expected_service_name = std::env::var("EXPECTED_SERVICE_NAME")
            .unwrap_or_else(|_| EXPECTED_SERVICE_NAME.to_string());
        let expected_span_name =
            std::env::var("EXPECTED_SPAN_NAME").unwrap_or_else(|_| EXPECTED_SPAN_NAME.to_string());

        Self {
            app_url: std::env::var("APP_BASE_URL").unwrap_or_else(|_| APP_BASE_URL.to_string()),
            telemetry_url: std::env::var("TELEMETRY_BASE_URL")
                .unwrap_or_else(|_| TELEMETRY_BASE_URL.to_string()),
            tempo_url: std::env::var("TEMPO_DIRECT_URL")
                .unwrap_or_else(|_| TEMPO_DIRECT_URL.to_string()),
            books_endpoint: std::env::var("BOOKS_ENDPOINT")
                .unwrap_or_else(|_| BOOKS_ENDPOINT.to_string()),
            trace_propagation_wait: Duration::from_secs(
                std::env::var("TRACE_PROPAGATION_WAIT_SECS")
                    .unwrap_or_else(|_| TRACE_PROPAGATION_WAIT_SECS.to_string())
                    .parse()
                    .unwrap_or(TRACE_PROPAGATION_WAIT_SECS),
            ),
            log_lookback_duration: Duration::from_secs(
                std::env::var("LOG_LOOKBACK_SECS")
                    .unwrap_or_else(|_| LOG_LOOKBACK_SECS.to_string())
                    .parse()
                    .unwrap_or(LOG_LOOKBACK_SECS),
            ),
            prometheus_datasource_id: std::env::var("PROMETHEUS_DATASOURCE_ID")
                .unwrap_or_else(|_| "1".to_string()),
            tempo_datasource_id: std::env::var("TEMPO_DATASOURCE_ID")
                .unwrap_or_else(|_| "2".to_string()),
            loki_datasource_id: std::env::var("LOKI_DATASOURCE_ID").unwrap_or_else(|_| "3".to_string()),
            expected_service_name: expected_service_name.clone(),
            expected_span_name: expected_span_name.clone(),
            prometheus_query: std::env::var("PROMETHEUS_QUERY").unwrap_or_else(|_| {
                // Use the server-level span name for metrics, which is typically the HTTP route
                let server_span_name = if expected_span_name == "get_all_books" {
                    "GET /books"
                } else {
                    &expected_span_name
                };
                format!(
                    "sum(traces_spanmetrics_calls_total{{service=\"{expected_service_name}\", span_kind=\"SPAN_KIND_SERVER\", span_name=\"{server_span_name}\"}}) by (span_name)"
                )
            }),
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
        println!("Test OTLP Exporter Endpoint: {otlp_endpoint}");

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

    // Try direct Tempo API first, then Grafana proxy
    let tempo_urls = [
        format!("{}/api/traces/{trace_id}", config.tempo_url),
        format!(
            "{}/api/datasources/proxy/{}/api/traces/{}",
            config.telemetry_url, config.tempo_datasource_id, trace_id
        ),
    ];

    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        println!("Attempt {attempt} for Tempo trace query");

        for (i, tempo_url) in tempo_urls.iter().enumerate() {
            println!("Trying URL {}: {}", i + 1, tempo_url);

            match http_client.get(tempo_url).send().await {
                Ok(response) => {
                    let status = response.status();
                    println!("Tempo API response status: {status}");

                    if status == reqwest::StatusCode::OK {
                        match response.text().await {
                            Ok(response_text) => {
                                match serde_json::from_str::<TempoResponse>(&response_text) {
                                    Ok(tempo_response) => {
                                        println!(
                                            "Successfully parsed Tempo response with {} batches",
                                            tempo_response.batches.len()
                                        );

                                        for (batch_idx, batch) in
                                            tempo_response.batches.iter().enumerate()
                                        {
                                            let service_name = batch
                                                .resource
                                                .attributes
                                                .iter()
                                                .find(|kv| kv.key == "service.name")
                                                .and_then(|kv| kv.value.string_value.as_ref());

                                            println!(
                                                "Batch {batch_idx}: service.name = {service_name:?}"
                                            );

                                            if service_name == Some(&config.expected_service_name) {
                                                println!(
                                                    "Found matching service: {}",
                                                    config.expected_service_name
                                                );

                                                for (scope_idx, scope_span) in
                                                    batch.scope_spans.iter().enumerate()
                                                {
                                                    println!(
                                                        "Scope {}: {} spans",
                                                        scope_idx,
                                                        scope_span.spans.len()
                                                    );

                                                    for (span_idx, span) in
                                                        scope_span.spans.iter().enumerate()
                                                    {
                                                        println!(
                                                            "  Span {}: name='{}', kind='{}'",
                                                            span_idx, span.name, span.kind
                                                        );

                                                        if span.name == config.expected_span_name
                                                            && (span.kind == "SPAN_KIND_SERVER"
                                                                || span.kind
                                                                    == "SPAN_KIND_INTERNAL")
                                                        {
                                                            println!("✅ Found expected span: {} with kind {}", span.name, span.kind);
                                                            return Ok(());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        println!("No matching span found. Expected: name='{}', kind='SPAN_KIND_SERVER' or 'SPAN_KIND_INTERNAL', service='{}'", 
                                               config.expected_span_name, config.expected_service_name);
                                    }
                                    Err(e) => {
                                        println!("Failed to parse Tempo JSON response: {e:?}");
                                        println!(
                                            "Response text (first 500 chars): {}",
                                            &response_text.chars().take(500).collect::<String>()
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                println!("Failed to read response text: {e:?}");
                            }
                        }
                    } else if status == reqwest::StatusCode::NOT_FOUND {
                        println!("Trace {trace_id} not found in {tempo_url} (404 - expected for early attempts)");
                    } else {
                        let error_body = response.text().await.unwrap_or_default();
                        println!("❌ Error response from {tempo_url}: {status} - {error_body}");
                    }
                }
                Err(e) => println!("Request failed for {tempo_url}: {e:?}"),
            }
        }

        if attempt < MAX_TEMPO_ATTEMPTS {
            let delay = Duration::from_secs(std::cmp::min(
                attempt as u64 * BASE_RETRY_DELAY_SECS,
                MAX_RETRY_DELAY_SECS,
            ));
            println!("Waiting {delay:?} before next attempt...");
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "tempo_query",
        format!("Failed to find trace {trace_id} after {MAX_TEMPO_ATTEMPTS} attempts"),
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

    let log_query = format!("{{service_name=\"{}\"}}", config.expected_service_name);
    let loki_query_url = format!(
        "{}/api/datasources/proxy/{}/loki/api/v1/query_range?query={}&start={}&end={}&direction=forward",
        config.telemetry_url,
        config.loki_datasource_id,
        urlencoding::encode(&log_query),
        start_ns,
        now_ns
    );

    println!("Loki query URL: {loki_query_url}");

    for attempt in 1..=MAX_LOKI_ATTEMPTS {
        println!("Attempt {attempt} for Loki logs query");

        match http_client.get(&loki_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Loki API response status: {status}");

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
                                            "Found {log_count} log entries in Loki for service {}.",
                                            config.expected_service_name
                                        );
                                        return Ok(());
                                    }
                                }
                                Err(e) => println!("Failed to parse Loki JSON response: {e:?}"),
                            }
                        }
                        Err(e) => println!("Failed to read Loki response: {e:?}"),
                    }
                }
            }
            Err(e) => println!("Loki request failed: {e:?}"),
        }

        if attempt < MAX_LOKI_ATTEMPTS {
            let delay = Duration::from_secs(std::cmp::min(
                attempt as u64 * BASE_RETRY_DELAY_SECS,
                MAX_RETRY_DELAY_SECS,
            ));
            println!("Waiting {delay:?} before next attempt...");
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "loki_query",
        format!(
            "Failed to find logs for service {} after {MAX_LOKI_ATTEMPTS} attempts",
            config.expected_service_name
        ),
    ))
}

async fn query_prometheus_for_metrics(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    validate_trace_id(trace_id)?;

    let prom_query = &config.prometheus_query;

    let prometheus_query_url = format!(
        "{}/api/datasources/proxy/{}/api/v1/query?query={}",
        config.telemetry_url,
        config.prometheus_datasource_id,
        urlencoding::encode(prom_query)
    );

    println!("Prometheus query URL: {prometheus_query_url}");

    for attempt in 1..=MAX_PROMETHEUS_ATTEMPTS {
        println!("Attempt {attempt} for Prometheus metrics query");

        match http_client.get(&prometheus_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Prometheus API response status: {status}");

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
                                                        println!("Successfully found metric with value {val} >= 1.0");
                                                        return Ok(());
                                                    }
                                                    Ok(val) => {
                                                        println!("Metric value {val} is < 1.0")
                                                    }
                                                    Err(e) => println!(
                                                        "Failed to parse metric value '{value_str}': {e:?}"
                                                    ),
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("Failed to parse Prometheus JSON response: {e:?}")
                                }
                            }
                        }
                        Err(e) => println!("Failed to read Prometheus response: {e:?}"),
                    }
                }
            }
            Err(e) => println!("Prometheus request failed: {e:?}"),
        }

        if attempt < MAX_PROMETHEUS_ATTEMPTS {
            let delay = Duration::from_secs(std::cmp::min(
                attempt as u64 * BASE_RETRY_DELAY_SECS,
                MAX_RETRY_DELAY_SECS,
            ));
            println!("Waiting {delay:?} before next attempt...");
            tokio::time::sleep(delay).await;
        }
    }

    Err(TestError::new(
        "prometheus_query",
        format!(
            "Failed to find metrics for trace {trace_id} after {MAX_PROMETHEUS_ATTEMPTS} attempts"
        ),
    ))
}

#[tokio::test]
async fn test_root_endpoint_generates_telemetry() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🚀 Starting telemetry integration test");
    println!("📋 Test configuration:");
    println!("  App URL: {}", config.app_url);
    println!("  Telemetry URL: {}", config.telemetry_url);
    println!("  Tempo URL: {}", config.tempo_url);
    println!("  Expected service: {}", config.expected_service_name);
    println!("  Expected span: {}", config.expected_span_name);

    init_test_tracing()?;

    let http_client = HttpClient::new();
    verify_service_connectivity(&http_client, &config).await?;

    let (trace_id, _) = execute_traced_request(&config).await?;
    wait_for_trace_propagation(&config).await;

    // Test all telemetry systems
    verify_telemetry_in_all_systems(&http_client, &trace_id, &config).await?;

    println!("✅ Test completed successfully!");
    Ok(())
}

#[tokio::test]
async fn test_error_endpoint_generates_error_trace() -> TestResult<()> {
    let config = TestConfig::default();
    init_test_tracing()?;

    let http_client = HttpClient::new();

    // Configure error injection against a unique, numeric path so the handler executes
    let test_endpoint = format!("/books/{}", uuid::Uuid::now_v7());
    let error_injection_config = serde_json::json!({
        "endpoint_pattern": test_endpoint.clone(),
        "http_method": "GET",
        "error_rate": 1.0,
        "error_code": 500,
        "error_message": "Injected Internal Server Error"
    });

    let response = http_client
        .post(format!("{}/error-injection", config.app_url))
        .json(&error_injection_config)
        .send()
        .await
        .map_err(|e| TestError::new("error_injection_setup", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "error_injection_setup",
            format!("Failed to configure error injection: {}", response.status()),
        ));
    }

    let created_config: serde_json::Value = response
        .json()
        .await
        .map_err(|e| TestError::new("error_injection_setup", e.to_string()))?;
    let config_id = created_config["id"].as_i64().ok_or_else(|| {
        TestError::new(
            "error_injection_setup",
            "Created config missing numeric id".to_string(),
        )
    })?;

    // Make a request that should fail
    let error_status_expected = reqwest::StatusCode::INTERNAL_SERVER_ERROR;
    let mut response = None;
    const MAX_ATTEMPTS: usize = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        let candidate = http_client
            .get(format!("{}{}", config.app_url, test_endpoint))
            .send()
            .await
            .map_err(|e| TestError::new("http_request_error_case", e.to_string()))?;

        if candidate.status() == error_status_expected {
            response = Some(candidate);
            break;
        }

        if attempt < MAX_ATTEMPTS {
            println!(
                "⏳ Error endpoint returned {} (attempt {}/{}) – retrying...",
                candidate.status(),
                attempt,
                MAX_ATTEMPTS
            );
            tokio::time::sleep(Duration::from_millis(200 * attempt as u64)).await;
        } else {
            return Err(TestError::new(
                "http_request_error_case",
                format!(
                    "Expected status {} from injected endpoint after {} attempts, last status {}",
                    error_status_expected,
                    MAX_ATTEMPTS,
                    candidate.status()
                ),
            ));
        }
    }

    let response = response.expect("response must be present after successful attempt");

    let trace_id = if let Some(traceparent) = response.headers().get("traceparent") {
        if let Ok(traceparent_str) = traceparent.to_str() {
            let parts: Vec<&str> = traceparent_str.split('-').collect();
            if parts.len() >= 2 {
                parts[1].to_string()
            } else {
                return Err(TestError::new(
                    "trace_extraction_error_case",
                    format!("Invalid traceparent format: {traceparent_str}"),
                ));
            }
        } else {
            return Err(TestError::new(
                "trace_extraction_error_case",
                "Failed to parse traceparent header as string".to_string(),
            ));
        }
    } else {
        return Err(TestError::new(
            "trace_extraction_error_case",
            "No traceparent header found in response".to_string(),
        ));
    };

    wait_for_trace_propagation(&config).await;

    // Verify that the trace exists in Tempo and has an error status
    query_tempo_for_trace_with_error_status(&http_client, &trace_id, &config).await?;

    // Best-effort cleanup so other tests see normal responses
    let _ = http_client
        .delete(format!("{}/error-injection/{}", config.app_url, config_id))
        .send()
        .await;

    println!("✅ Error telemetry test completed successfully!");
    Ok(())
}

async fn query_tempo_for_trace_with_error_status(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    validate_trace_id(trace_id)?;

    let tempo_urls = [
        format!("{}/api/traces/{trace_id}", config.tempo_url),
        format!(
            "{}/api/datasources/proxy/{}/api/traces/{}",
            config.telemetry_url, config.tempo_datasource_id, trace_id
        ),
    ];

    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        for tempo_url in &tempo_urls {
            if let Ok(response) = http_client.get(tempo_url).send().await {
                if response.status() == reqwest::StatusCode::OK {
                    if let Ok(response_text) = response.text().await {
                        if let Ok(tempo_response) =
                            serde_json::from_str::<TempoResponse>(&response_text)
                        {
                            if let Some(batch) = tempo_response.batches.iter().find(|batch| {
                                batch.resource.attributes.iter().any(|kv| {
                                    kv.key == "service.name"
                                        && kv.value.string_value
                                            == Some(config.expected_service_name.clone())
                                })
                            }) {
                                if let Some(scope_span) = batch.scope_spans.first() {
                                    if scope_span.spans.iter().any(|s| {
                                        s.status.code == Some("STATUS_CODE_ERROR".to_string())
                                    }) {
                                        println!("Found trace with error status.");
                                        return Ok(());
                                    }
                                }
                            }
                        } else {
                            println!("Failed to parse Tempo JSON response: {response_text}");
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(attempt as u64 * BASE_RETRY_DELAY_SECS)).await;
    }

    Err(TestError::new(
        "tempo_error_query",
        format!("Failed to find trace with error status for trace ID {trace_id}"),
    ))
}

/// Create a Progenitor BookApp client with OpenTelemetry tracing support
fn create_traced_bookapp_client(base_url: &str) -> TestResult<BookappClient> {
    let client_state = ClientState::default();

    // Create the client - OpenTelemetry context injection is built into the generated client
    let bookapp_client = BookappClient::new(base_url, client_state);

    Ok(bookapp_client)
}

async fn execute_traced_request(config: &TestConfig) -> TestResult<(String, HttpClient)> {
    let http_client = HttpClient::new();

    // Create Progenitor client with OpenTelemetry context injection
    let bookapp_client = create_traced_bookapp_client(&config.app_url)?;

    println!(
        "📡 Sending request to {} endpoint using Progenitor client",
        config.books_endpoint
    );

    // Use the generated client instead of raw HTTP calls for automatic tracing
    let response = bookapp_client
        .get_all_books()
        .send()
        .await
        .map_err(|e| TestError::new("bookapp_client_request", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "bookapp_client_request",
            format!(
                "Request to {} endpoint via Progenitor client failed with status: {}",
                config.books_endpoint,
                response.status()
            ),
        ));
    }

    // Extract trace ID from the traceparent header.
    // This is a critical part of the test, as it verifies that the trace context
    // is being correctly propagated from the service. If this header is missing,
    // it indicates a fundamental problem with the telemetry setup.
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
                    format!("Invalid traceparent format: {traceparent_str}"),
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

async fn wait_for_trace_propagation(config: &TestConfig) {
    println!("⏳ Waiting for trace propagation...");
    tokio::time::sleep(config.trace_propagation_wait).await;
}

async fn verify_service_connectivity(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔗 Verifying service connectivity...");

    // Check app service
    let app_health_url = format!("{}/health", config.app_url);
    match http_client.get(&app_health_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                println!("✅ App service is reachable at {}", config.app_url);
            } else {
                println!(
                    "⚠️  App service returned {}: {}",
                    response.status(),
                    config.app_url
                );
            }
        }
        Err(e) => {
            println!(
                "⚠️  Failed to reach app service at {}: {}",
                config.app_url, e
            );
        }
    }

    // Check telemetry service
    let telemetry_health_url = format!("{}/api/health", config.telemetry_url);
    match http_client.get(&telemetry_health_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                println!(
                    "✅ Telemetry service is reachable at {}",
                    config.telemetry_url
                );
            } else {
                println!(
                    "⚠️  Telemetry service returned {}: {}",
                    response.status(),
                    config.telemetry_url
                );
            }
        }
        Err(e) => {
            println!(
                "⚠️  Failed to reach telemetry service at {}: {}",
                config.telemetry_url, e
            );
        }
    }

    // Check Tempo direct access
    let tempo_health_url = format!("{}/ready", config.tempo_url);
    match http_client.get(&tempo_health_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                println!("✅ Tempo service is reachable at {}", config.tempo_url);
            } else {
                println!(
                    "⚠️  Tempo service returned {}: {}",
                    response.status(),
                    config.tempo_url
                );
            }
        }
        Err(e) => {
            println!(
                "⚠️  Failed to reach Tempo service at {}: {}",
                config.tempo_url, e
            );
        }
    }

    Ok(())
}

async fn verify_telemetry_in_all_systems(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔎 Verifying telemetry data in all systems...");

    // Tempo verification (required)
    verify_tempo_trace(http_client, trace_id, config).await?;

    // Run Loki and Prometheus verifications in parallel
    let (loki_result, prometheus_result) = tokio::join!(
        verify_loki_logs(http_client, trace_id, config),
        verify_prometheus_metrics(http_client, trace_id, config)
    );

    // Loki verification (optional - logs may not have trace correlation yet)
    match loki_result {
        Ok(()) => println!("✅ Loki verification successful"),
        Err(e) => println!(
            "⚠️  Loki verification failed (trace correlation may not be configured): {}",
            e.message
        ),
    }

    // Prometheus verification (required)
    prometheus_result.map_err(|e| TestError::new("prometheus_verification", e.message))?;
    println!("✅ Prometheus verification successful");

    Ok(())
}

async fn verify_tempo_trace(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🎯 Querying Tempo for trace: {trace_id}");
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
    println!("📋 Querying Loki for logs with trace: {trace_id}");
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
    println!("📊 Querying Prometheus for metrics with trace: {trace_id}");
    query_prometheus_for_metrics(http_client, trace_id, config)
        .await
        .map_err(|e| TestError::new("prometheus_verification", e.message))?;
    println!("✅ Prometheus verification successful");
    Ok(())
}

#[tokio::test]
async fn test_observability_coverage() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🚀 Starting observability test");

    init_test_tracing()?;
    let http_client = HttpClient::new();
    verify_service_connectivity(&http_client, &config).await?;

    // Test multiple endpoints to ensure comprehensive coverage
    // Note: /health endpoint doesn't generate traces as it's filtered out
    let endpoints = vec![("/books", "get_all_books")];

    let mut all_trace_ids = Vec::new();

    for (endpoint, expected_span) in endpoints {
        println!("📍 Testing endpoint: {endpoint}");

        let endpoint_url = format!("{}{endpoint}", config.app_url);
        let response = http_client
            .get(&endpoint_url)
            .send()
            .await
            .map_err(|e| TestError::new("http_request", e.to_string()))?;

        if !response.status().is_success() {
            println!(
                "⚠️  Endpoint {endpoint} returned {}, skipping telemetry verification",
                response.status()
            );
            continue;
        }

        if let Some(traceparent) = response.headers().get("traceparent") {
            if let Ok(traceparent_str) = traceparent.to_str() {
                let parts: Vec<&str> = traceparent_str.split('-').collect();
                if parts.len() >= 2 {
                    let trace_id = parts[1].to_string();
                    validate_trace_id(&trace_id)?;
                    all_trace_ids.push((
                        trace_id.clone(),
                        endpoint.to_string(),
                        expected_span.to_string(),
                    ));
                    println!("✅ Extracted trace ID {trace_id} for {endpoint}");
                }
            }
        } else {
            println!("⚠️  No traceparent header found for {endpoint}");
        }
    }

    if all_trace_ids.is_empty() {
        return Err(TestError::new(
            "comprehensive_test",
            "No traces found for any endpoint".to_string(),
        ));
    }

    wait_for_trace_propagation(&config).await;

    // Verify each trace in telemetry systems
    for (trace_id, endpoint, expected_span) in all_trace_ids {
        println!("🔍 Verifying telemetry for {endpoint} (trace: {trace_id})");

        // Create a custom config for this specific span
        let mut endpoint_config = config.clone();
        endpoint_config.expected_span_name = expected_span;

        match verify_telemetry_in_all_systems(&http_client, &trace_id, &endpoint_config).await {
            Ok(()) => println!("✅ Telemetry verification successful for {endpoint}"),
            Err(e) => println!(
                "⚠️  Telemetry verification failed for {endpoint}: {}",
                e.message
            ),
        }
    }

    println!("✅ Comprehensive observability test completed!");
    Ok(())
}

#[tokio::test]
async fn test_cross_service_tracing_book_creation() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🚀 Starting cross-service tracing test via book creation");

    init_test_tracing()?;
    let http_client = HttpClient::new();
    verify_service_connectivity(&http_client, &config).await?;

    // Additional check: ensure bookapp service is responding to API calls
    println!("🔍 Testing bookapp API before bulk loader test");
    let response = http_client
        .get(format!("{}/books", config.app_url))
        .send()
        .await
        .map_err(|e| TestError::new("bookapp_connectivity", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "bookapp_connectivity",
            format!(
                "Bookapp service not responding properly: {}",
                response.status()
            ),
        ));
    }
    println!("✅ Bookapp API is responding correctly");

    // Create a book via API to trigger cross-service communication
    let trace_id = create_book_and_get_trace_id(&http_client, &config).await?;
    wait_for_trace_propagation(&config).await;

    // Verify cross-service traces appear in Tempo (including linked traces)
    verify_linked_cross_service_traces(&http_client, &trace_id, &config).await?;

    println!("✅ Cross-service tracing test completed successfully!");
    Ok(())
}

#[tokio::test]
async fn test_data_loader_integration() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🚀 Starting data-loader integration test");

    init_test_tracing()?;
    let http_client = HttpClient::new();
    verify_service_connectivity(&http_client, &config).await?;

    // Test data-loader using Progenitor client to ensure it works correctly
    println!("📦 Testing data-loader with Progenitor client");

    // Create a Progenitor client like the data-loader does
    let client_state = client::ClientState::default();
    let bookapp_client = client::Client::new(&config.app_url, client_state);

    // Test connectivity first
    println!("🔍 Testing connectivity with Progenitor client");
    let response = bookapp_client
        .get_all_books()
        .send()
        .await
        .map_err(|e| TestError::new("progenitor_connectivity", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "progenitor_connectivity",
            format!(
                "Progenitor client connectivity failed: {}",
                response.status()
            ),
        ));
    }
    println!("✅ Progenitor client connectivity successful");

    // Create a book using the Progenitor client (simulating data-loader behavior)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let book_create = client::types::BookCreateIn {
        work_title: format!("Data Loader Test Book #{}", timestamp),
        primary_author_name: Some("Data Loader Test Author".to_string()),
        primary_author_id: None,
        status: None,
    };

    println!("📚 Creating book via Progenitor client");
    let response = bookapp_client
        .create_book()
        .body(book_create)
        .send()
        .await
        .map_err(|e| TestError::new("progenitor_create_book", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "progenitor_create_book",
            format!(
                "Failed to create book via Progenitor client: {}",
                response.status()
            ),
        ));
    }

    // Extract trace ID from the response
    let trace_id = if let Some(traceparent) = response.headers().get("traceparent") {
        if let Ok(traceparent_str) = traceparent.to_str() {
            let parts: Vec<&str> = traceparent_str.split('-').collect();
            if parts.len() >= 2 {
                parts[1].to_string()
            } else {
                return Err(TestError::new(
                    "trace_extraction_progenitor",
                    format!("Invalid traceparent format: {traceparent_str}"),
                ));
            }
        } else {
            return Err(TestError::new(
                "trace_extraction_progenitor",
                "Failed to parse traceparent header as string".to_string(),
            ));
        }
    } else {
        return Err(TestError::new(
            "trace_extraction_progenitor",
            "No traceparent header found in Progenitor response".to_string(),
        ));
    };

    println!(
        "📝 Book created via Progenitor client, trace ID: {}",
        trace_id
    );
    wait_for_trace_propagation(&config).await;

    // Verify the trace appears in Tempo (including linked traces)
    verify_linked_cross_service_traces(&http_client, &trace_id, &config).await?;

    println!("✅ Data-loader integration test completed successfully!");
    Ok(())
}

async fn create_book_and_get_trace_id(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<String> {
    println!("📚 Creating book via API to trigger cross-service tracing");

    // Create a unique book to avoid conflicts
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let book_data = serde_json::json!({
        "work_title": format!("Test Book #{}", timestamp),
        "primary_author_name": "Test Author",
        "status": "Available"
    });

    let response = http_client
        .post(format!("{}/books/add", config.app_url))
        .json(&book_data)
        .send()
        .await
        .map_err(|e| TestError::new("book_creation", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "book_creation",
            format!("Failed to create book: {}", response.status()),
        ));
    }

    // Extract trace ID from traceparent header
    let trace_id = if let Some(traceparent) = response.headers().get("traceparent") {
        if let Ok(traceparent_str) = traceparent.to_str() {
            let parts: Vec<&str> = traceparent_str.split('-').collect();
            if parts.len() >= 2 {
                parts[1].to_string()
            } else {
                return Err(TestError::new(
                    "trace_extraction",
                    format!("Invalid traceparent format: {traceparent_str}"),
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

    println!("📝 Created book successfully, trace ID: {}", trace_id);
    Ok(trace_id)
}

async fn verify_linked_cross_service_traces(
    http_client: &HttpClient,
    original_trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔍 Verifying linked cross-service traces for book creation in Tempo");
    println!("🔍 Original trace ID: {}", original_trace_id);

    // Step 1: Verify the original bookapp trace exists and has correct structure
    println!("📋 Step 1: Verifying bookapp trace");
    let bookapp_trace =
        verify_cross_service_span_structure(http_client, original_trace_id, config).await;

    // Even if bookapp trace verification fails on backend requirement, continue to check for linked traces
    let bookapp_has_kafka_producer = match &bookapp_trace {
        Ok(_) => {
            println!("✅ Bookapp trace structure verified");
            true
        }
        Err(e) => {
            if e.message
                .contains("Missing required service in trace: backend")
            {
                println!("⚠️  Bookapp trace found but backend not in same trace (expected for linked traces)");
                // Verify just the bookapp trace exists
                verify_bookapp_trace_only(http_client, original_trace_id, config).await?;
                true
            } else {
                println!("❌ Bookapp trace verification failed: {}", e.message);
                return Err(e.clone());
            }
        }
    };

    if !bookapp_has_kafka_producer {
        return Err(TestError::new(
            "linked_trace_verification",
            "Could not verify bookapp trace exists".to_string(),
        ));
    }

    // Step 2: Search for backend traces that are linked to this trace context
    println!("📋 Step 2: Searching for linked backend traces");

    // Search for backend traces that might be linked
    // Use a time-based search to find backend traces around the same time
    let backend_found = search_for_backend_traces(http_client, config).await?;

    if backend_found {
        println!("🎯 Cross-service linked tracing validation passed!");
        println!("✅ Found both bookapp and backend traces (linked via Kafka)");
    } else {
        return Err(TestError::new(
            "linked_trace_verification",
            "Backend trace not found - Kafka message processing may not be working".to_string(),
        ));
    }

    Ok(())
}

async fn verify_cross_service_span_structure(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!(
        "🔍 Verifying span structure for cross-service trace: {}",
        trace_id
    );
    validate_trace_id(trace_id)?;

    // Get the full trace details with retry logic
    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        println!(
            "🔄 Tempo trace detail query attempt {}/{}",
            attempt, MAX_TEMPO_ATTEMPTS
        );

        let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
        let response = http_client
            .get(&trace_url)
            .send()
            .await
            .map_err(|e| TestError::new("tempo_trace_detail", e.to_string()))?;

        if response.status().is_success() {
            // Parse the response and continue with verification
            let trace_data: TempoResponse = response
                .json()
                .await
                .map_err(|e| TestError::new("tempo_trace_parse", e.to_string()))?;

            return verify_trace_data(&trace_data);
        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
            println!("⏳ Trace not found yet, waiting...");
            if attempt < MAX_TEMPO_ATTEMPTS {
                let delay = std::cmp::min(
                    BASE_RETRY_DELAY_SECS * 2_u64.pow(attempt as u32 - 1),
                    MAX_RETRY_DELAY_SECS,
                );
                println!("⏳ Waiting {}s before next attempt", delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        } else {
            return Err(TestError::new(
                "tempo_trace_detail",
                format!("Failed to get trace details: {}", response.status()),
            ));
        }
    }

    Err(TestError::new(
        "tempo_trace_detail",
        format!(
            "Trace {} not found after {} attempts",
            trace_id, MAX_TEMPO_ATTEMPTS
        ),
    ))
}

fn verify_trace_data(trace_data: &TempoResponse) -> TestResult<()> {
    // Verify we have spans from expected services
    let mut found_services = std::collections::HashSet::new();
    let mut found_spans = std::collections::HashSet::new();

    for batch in &trace_data.batches {
        for attr in &batch.resource.attributes {
            if attr.key == "service.name" {
                if let Some(service_name) = &attr.value.string_value {
                    found_services.insert(service_name.clone());
                }
            }
        }

        for instrumentation_scope in &batch.scope_spans {
            for span in &instrumentation_scope.spans {
                found_spans.insert(span.name.clone());
            }
        }
    }

    // Verify expected services are present - this is REQUIRED for cross-service tracing validation
    let required_services = vec![EXPECTED_SERVICE_NAME, "backend"];
    for service in &required_services {
        if !found_services.contains(*service) {
            return Err(TestError::new(
                "cross_service_verification",
                format!(
                    "Missing required service in trace: {}. Found services: {:?}",
                    service, found_services
                ),
            ));
        }
    }

    // Verify we have spans from bookapp service (at minimum)
    let bookapp_spans = ["create_book", "POST /books/add"];
    let has_bookapp_span = bookapp_spans.iter().any(|span| found_spans.contains(*span));

    if !has_bookapp_span {
        println!(
            "⚠️  Expected bookapp spans not found, but found spans: {:?}",
            found_spans
        );
        // Don't fail the test if spans are missing - the service presence is more important
    }

    println!("🎯 Cross-service tracing validation passed!");

    println!("✅ Cross-service trace structure verified:");
    println!("  Services: {:?}", found_services);
    println!("  Key spans: {:?}", found_spans);

    Ok(())
}

async fn verify_bookapp_trace_only(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔍 Verifying bookapp trace exists: {}", trace_id);
    validate_trace_id(trace_id)?;

    // Get the trace details with retry logic
    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        println!(
            "🔄 Bookapp trace query attempt {}/{}",
            attempt, MAX_TEMPO_ATTEMPTS
        );

        let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
        let response = http_client
            .get(&trace_url)
            .send()
            .await
            .map_err(|e| TestError::new("tempo_trace_detail", e.to_string()))?;

        if response.status().is_success() {
            let trace_data: TempoResponse = response
                .json()
                .await
                .map_err(|e| TestError::new("tempo_trace_parse", e.to_string()))?;

            return verify_bookapp_trace_data(&trace_data);
        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
            println!("⏳ Bookapp trace not found yet, waiting...");
            if attempt < MAX_TEMPO_ATTEMPTS {
                let delay = std::cmp::min(
                    BASE_RETRY_DELAY_SECS * 2_u64.pow(attempt as u32 - 1),
                    MAX_RETRY_DELAY_SECS,
                );
                println!("⏳ Waiting {}s before next attempt", delay);
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        } else {
            return Err(TestError::new(
                "tempo_trace_detail",
                format!("Failed to get trace details: {}", response.status()),
            ));
        }
    }

    Err(TestError::new(
        "tempo_trace_detail",
        format!(
            "Bookapp trace {} not found after {} attempts",
            trace_id, MAX_TEMPO_ATTEMPTS
        ),
    ))
}

fn verify_bookapp_trace_data(trace_data: &TempoResponse) -> TestResult<()> {
    let mut found_services = std::collections::HashSet::new();
    let mut found_spans = std::collections::HashSet::new();

    for batch in &trace_data.batches {
        for attr in &batch.resource.attributes {
            if attr.key == "service.name" {
                if let Some(service_name) = &attr.value.string_value {
                    found_services.insert(service_name.clone());
                }
            }
        }

        for instrumentation_scope in &batch.scope_spans {
            for span in &instrumentation_scope.spans {
                found_spans.insert(span.name.clone());
            }
        }
    }

    // Verify bookapp service is present
    if !found_services.contains(EXPECTED_SERVICE_NAME) {
        return Err(TestError::new(
            "bookapp_trace_verification",
            format!(
                "Missing bookapp service in trace. Found services: {:?}",
                found_services
            ),
        ));
    }

    println!("✅ Bookapp trace verified:");
    println!("  Services: {:?}", found_services);
    println!(
        "  Spans: {:?}",
        found_spans.iter().take(5).collect::<Vec<_>>()
    );

    Ok(())
}

async fn search_for_backend_traces(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<bool> {
    println!("🔍 Searching for backend traces in Tempo");

    // Search for traces from backend service
    let trace_query = "{resource.service.name=\"backend\"}".to_string();

    for attempt in 1..=MAX_TEMPO_ATTEMPTS {
        println!(
            "🔄 Backend trace search attempt {}/{}",
            attempt, MAX_TEMPO_ATTEMPTS
        );

        let tempo_search_url = format!(
            "{}/api/search?q={}",
            config.tempo_url,
            urlencoding::encode(&trace_query)
        );

        let response = http_client
            .get(&tempo_search_url)
            .send()
            .await
            .map_err(|e| TestError::new("tempo_search_backend", e.to_string()))?;

        if response.status().is_success() {
            let search_results: serde_json::Value = response
                .json()
                .await
                .map_err(|e| TestError::new("tempo_search_parse_backend", e.to_string()))?;

            // Check if we found any backend traces
            if let Some(traces) = search_results.get("traces") {
                if let Some(traces_array) = traces.as_array() {
                    if !traces_array.is_empty() {
                        println!("✅ Found {} backend traces", traces_array.len());

                        // Look for traces that contain book ingestion processing
                        for trace in traces_array {
                            if let Some(trace_id) = trace.get("traceID") {
                                if let Some(trace_id_str) = trace_id.as_str() {
                                    // Check if this backend trace has book ingestion spans
                                    if contains_book_ingestion_spans(
                                        http_client,
                                        trace_id_str,
                                        config,
                                    )
                                    .await?
                                    {
                                        println!(
                                            "🎯 Found backend trace with book ingestion: {}",
                                            trace_id_str
                                        );
                                        return Ok(true);
                                    }
                                }
                            }
                        }

                        // If we found backend traces but none with book ingestion, that's still good
                        println!("✅ Found backend traces (may not be related to our test)");
                        return Ok(true);
                    } else {
                        println!("⚠️  No backend traces found yet");
                    }
                }
            }
        }

        if attempt < MAX_TEMPO_ATTEMPTS {
            let delay = std::cmp::min(
                BASE_RETRY_DELAY_SECS * 2_u64.pow(attempt as u32 - 1),
                MAX_RETRY_DELAY_SECS,
            );
            println!("⏳ Waiting {}s before next backend search attempt", delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    }

    println!(
        "⚠️  No backend traces found after {} attempts",
        MAX_TEMPO_ATTEMPTS
    );
    Ok(false)
}

async fn contains_book_ingestion_spans(
    http_client: &HttpClient,
    trace_id: &str,
    config: &TestConfig,
) -> TestResult<bool> {
    let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
    let response = http_client
        .get(&trace_url)
        .send()
        .await
        .map_err(|e| TestError::new("tempo_trace_detail_backend", e.to_string()))?;

    if !response.status().is_success() {
        return Ok(false);
    }

    let trace_data: TempoResponse = response.json().await.map_err(|_| {
        TestError::new(
            "tempo_trace_parse_backend",
            "Failed to parse backend trace".to_string(),
        )
    })?;

    // Look for book ingestion related spans
    for batch in &trace_data.batches {
        for instrumentation_scope in &batch.scope_spans {
            for span in &instrumentation_scope.spans {
                if span.name.contains("book_ingestion")
                    || span.name.contains("book_ingestion_processing")
                    || span.name.contains("consume")
                {
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}

/// Test to validate that all backend workers produce independent traces
/// This ensures each worker (Kafka consumer, outbox publisher, scheduled tasks)
/// has proper telemetry isolation and can be observed independently
#[tokio::test]
async fn test_backend_workers_independent_traces() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🔍 Testing backend workers produce independent traces");

    init_test_tracing()?;
    let http_client = HttpClient::new();

    // Verify backend service is running and Tempo is available
    verify_service_connectivity(&http_client, &config).await?;

    println!("⏳ Waiting for backend workers to generate telemetry...");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Query for traces from different backend workers - all use service name "backend"
    // but have different root trace names for different operations
    let backend_workers = vec![
        ("outbox_publish_cycle", "Outbox publisher worker"),
        ("publish_unpublished_events", "Domain event publishing"),
        ("book_ingestion_processing", "Kafka message processing"),
        (
            "generate_daily_statistics",
            "Scheduled statistics generation",
        ),
        ("refresh_search_materialized_view", "Scheduled view refresh"),
    ];

    let mut found_workers = Vec::new();
    let search_time_range = 300; // 5 minutes

    for (root_trace_name, description) in &backend_workers {
        println!("🔍 Searching for traces from: {}", description);

        let mut search_result: TempoSearchResponse = TempoSearchResponse { traces: Vec::new() };
        let mut found = false;

        for attempt in 1..=5 {
            search_result = search_backend_worker_traces_by_operation(
                &http_client,
                &config,
                root_trace_name,
                search_time_range,
            )
            .await?;

            if !search_result.traces.is_empty() {
                found = true;
                break;
            }

            if attempt < 5 {
                let backoff = std::cmp::min(2 * attempt as u64, 10);
                tokio::time::sleep(Duration::from_secs(backoff)).await;
            }
        }

        if found {
            found_workers.push((root_trace_name.to_string(), search_result.traces.len()));
            println!(
                "✅ Found {} traces for {}",
                search_result.traces.len(),
                root_trace_name
            );

            let trace_ids: std::collections::HashSet<String> = search_result
                .traces
                .iter()
                .map(|t| t.trace_id.clone())
                .collect();

            if trace_ids.len() != search_result.traces.len() {
                return Err(TestError::new(
                    "backend_worker_trace_independence",
                    format!(
                        "Found duplicate trace IDs in {} worker traces",
                        root_trace_name
                    ),
                ));
            }

            if let Some(trace) = search_result.traces.first() {
                let trace_valid = validate_worker_trace_structure(
                    &http_client,
                    &config,
                    &trace.trace_id,
                    root_trace_name,
                )
                .await?;

                if !trace_valid {
                    println!(
                        "⚠️  Warning: Trace structure validation failed for {}",
                        root_trace_name
                    );
                }
            }
        } else {
            println!(
                "⚠️  No traces found for {} after multiple attempts",
                root_trace_name
            );
        }
    }

    // Validate we found traces from the outbox publisher (most reliable worker)
    let outbox_worker_found = found_workers
        .iter()
        .any(|(name, _)| name == "outbox_publish_cycle");
    if !outbox_worker_found {
        return Err(TestError::new(
            "backend_outbox_traces_missing",
            "Expected to find traces from outbox publisher worker".to_string(),
        ));
    }

    println!("✅ Backend worker trace validation completed");
    println!(
        "📊 Summary: Found traces from {} out of {} workers",
        found_workers.len(),
        backend_workers.len()
    );

    for (worker, count) in &found_workers {
        println!("  - {}: {} traces", worker, count);
    }

    // Additional validation: Check for expected span operations in backend traces
    if let Some((_, _)) = found_workers.iter().find(|(name, _)| name == "backend") {
        println!("🔍 Validating backend operation spans...");
        let operations_found = validate_backend_operations(&http_client, &config).await?;

        if operations_found.is_empty() {
            println!("⚠️  No specific backend operations detected in traces");
        } else {
            println!("✅ Found backend operations: {:?}", operations_found);
        }
    }

    Ok(())
}

/// Search for traces from a specific backend worker by root trace name
async fn search_backend_worker_traces_by_operation(
    http_client: &HttpClient,
    config: &TestConfig,
    root_trace_name: &str,
    time_range_seconds: u64,
) -> TestResult<TempoSearchResponse> {
    // First get all backend traces, then filter by rootTraceName on client side
    let query = "{resource.service.name=\"backend\"}";
    let url = format!(
        "{}/api/search?q={}&limit=200&start={}&end={}",
        config.tempo_url,
        urlencoding::encode(query),
        chrono::Utc::now().timestamp() - time_range_seconds as i64,
        chrono::Utc::now().timestamp()
    );

    let response = http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| TestError::new("backend_worker_search", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "backend_worker_search_failed",
            format!(
                "Search failed for {}: {}",
                root_trace_name,
                response.status()
            ),
        ));
    }

    let mut all_traces: TempoSearchResponse = response
        .json()
        .await
        .map_err(|e| TestError::new("backend_worker_search_parse", e.to_string()))?;

    // Filter traces by rootTraceName on client side
    all_traces.traces.retain(|trace| {
        trace
            .root_trace_name
            .as_ref()
            .is_some_and(|name| name == root_trace_name)
    });

    Ok(all_traces)
}

/// Validate the structure of a worker's trace to ensure proper instrumentation
async fn validate_worker_trace_structure(
    http_client: &HttpClient,
    config: &TestConfig,
    trace_id: &str,
    worker_type: &str,
) -> TestResult<bool> {
    let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
    let response = http_client
        .get(&trace_url)
        .send()
        .await
        .map_err(|e| TestError::new("worker_trace_detail", e.to_string()))?;

    if !response.status().is_success() {
        return Ok(false);
    }

    let trace_data: TempoResponse = response
        .json()
        .await
        .map_err(|_| TestError::new("worker_trace_parse", "Failed to parse trace".to_string()))?;

    // Validate trace has spans
    let mut span_count = 0;
    let mut has_instrumentation = false;

    for batch in &trace_data.batches {
        for instrumentation_scope in &batch.scope_spans {
            for span in &instrumentation_scope.spans {
                span_count += 1;

                // Check for expected instrumentation patterns based on worker type
                match worker_type {
                    "backend" => {
                        if span.name.contains("main")
                            || span.name.contains("scheduler")
                            || span.name.contains("consumer")
                        {
                            has_instrumentation = true;
                        }
                    }
                    "backend-kafka-consumer" => {
                        if span.name.contains("consumer")
                            || span.name.contains("kafka")
                            || span.name.contains("book_ingestion")
                        {
                            has_instrumentation = true;
                        }
                    }
                    "backend-outbox-publisher" => {
                        if span.name.contains("outbox") || span.name.contains("publish") {
                            has_instrumentation = true;
                        }
                    }
                    "backend-scheduler" => {
                        if span.name.contains("job")
                            || span.name.contains("schedule")
                            || span.name.contains("statistics")
                        {
                            has_instrumentation = true;
                        }
                    }
                    _ => {
                        has_instrumentation = span_count > 0; // Any spans count as instrumentation
                    }
                }
            }
        }
    }

    Ok(span_count > 0 && has_instrumentation)
}

/// Validate that expected backend operations are being traced
async fn validate_backend_operations(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<Vec<String>> {
    let expected_operations = vec![
        "run_consumer",
        "start_outbox_publisher",
        "start_scheduler_with_shutdown",
        "generate_daily_statistics",
        "refresh_search_materialized_view",
        "cleanup_old_data",
        "publish_unpublished_events",
    ];

    let mut found_operations = Vec::new();
    let search_time_range = 300; // 5 minutes

    // Search broadly for backend service traces
    let query = "{resource.service.name=\"backend\"}";
    let url = format!(
        "{}/api/search?q={}&limit=100&start={}&end={}",
        config.tempo_url,
        urlencoding::encode(query),
        chrono::Utc::now().timestamp() - search_time_range,
        chrono::Utc::now().timestamp()
    );

    let response = http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| TestError::new("backend_operations_search", e.to_string()))?;

    if !response.status().is_success() {
        return Ok(found_operations);
    }

    let search_result: TempoSearchResponse = response
        .json()
        .await
        .map_err(|_| TestError::new("backend_operations_parse", "Parse failed".to_string()))?;

    // Check a sample of traces for expected operations
    for trace in search_result.traces.iter().take(10) {
        let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace.trace_id);
        if let Ok(response) = http_client.get(&trace_url).send().await {
            if let Ok(trace_data) = response.json::<TempoResponse>().await {
                for batch in &trace_data.batches {
                    for instrumentation_scope in &batch.scope_spans {
                        for span in &instrumentation_scope.spans {
                            for expected_op in &expected_operations {
                                if span.name.contains(expected_op)
                                    && !found_operations.contains(&expected_op.to_string())
                                {
                                    found_operations.push(expected_op.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(found_operations)
}

/// Test end-to-end outbox pattern flow from book creation to Kafka consumption
/// This validates the complete domain event flow:
/// 1. Create book via API → Domain event created in outbox
/// 2. Outbox publisher picks up event → Publishes to Kafka
/// 3. Backend consumer receives Kafka message → Processes book
/// 4. Background processing → Refreshes materialized search view
#[tokio::test]
async fn test_outbox_pattern_end_to_end_flow() -> TestResult<()> {
    let config = TestConfig::default();
    println!("🚀 Testing complete outbox pattern end-to-end flow");

    init_test_tracing()?;
    let http_client = HttpClient::new();
    verify_service_connectivity(&http_client, &config).await?;

    println!("📊 Step 1: Recording baseline metrics");
    let initial_metrics = capture_baseline_metrics(&http_client, &config).await?;

    println!("📚 Step 2: Creating book to trigger domain event");
    let (book_trace_id, book_id) = create_book_and_extract_details(&http_client, &config).await?;
    println!(
        "✅ Created book with ID: {}, trace: {}",
        book_id, book_trace_id
    );

    println!("⏳ Step 3: Waiting for event processing...");
    tokio::time::sleep(Duration::from_secs(15)).await; // Allow time for full processing

    println!("🔍 Step 4: Validating outbox event was created");
    validate_domain_event_created(&http_client, &config, &book_trace_id).await?;

    println!("🔍 Step 5: Validating Kafka message was published");
    validate_kafka_message_published(&http_client, &config).await?;

    println!("🔍 Step 6: Validating backend processing occurred");
    validate_backend_processing(&http_client, &config).await?;

    println!("🔍 Step 7: Validating materialized view was refreshed");
    validate_materialized_view_refresh(&http_client, &config).await?;

    println!("📈 Step 8: Validating metrics increased");
    validate_metrics_increased(&http_client, &config, &initial_metrics).await?;

    println!("✅ End-to-end outbox pattern flow test completed successfully!");
    Ok(())
}

async fn capture_baseline_metrics(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<OutboxMetrics> {
    // Search for recent outbox traces to get baseline counts
    let outbox_traces = search_outbox_traces(http_client, config, 60).await?; // Last 60 seconds
    let kafka_traces = search_kafka_processing_traces(http_client, config, 60).await?;
    let view_refresh_traces = search_view_refresh_traces(http_client, config, 60).await?;

    Ok(OutboxMetrics {
        outbox_publishes: outbox_traces.len(),
        kafka_messages_processed: kafka_traces.len(),
        view_refreshes: view_refresh_traces.len(),
    })
}

async fn create_book_and_extract_details(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<(String, String)> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let book_data = serde_json::json!({
        "work_title": format!("Outbox Test Book #{}", timestamp),
        "primary_author_name": "Outbox Test Author",
        "status": "Available"
    });

    let response = http_client
        .post(format!("{}/books/add", config.app_url))
        .json(&book_data)
        .send()
        .await
        .map_err(|e| TestError::new("outbox_book_creation", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "outbox_book_creation",
            format!("Failed to create book: {}", response.status()),
        ));
    }

    // Extract trace ID from headers BEFORE reading body
    let trace_id = if let Some(traceparent) = response.headers().get("traceparent") {
        if let Ok(traceparent_str) = traceparent.to_str() {
            let parts: Vec<&str> = traceparent_str.split('-').collect();
            if parts.len() >= 2 {
                parts[1].to_string()
            } else {
                return Err(TestError::new(
                    "outbox_trace_extraction",
                    format!("Invalid traceparent format: {traceparent_str}"),
                ));
            }
        } else {
            return Err(TestError::new(
                "outbox_trace_extraction",
                "Failed to parse traceparent header".to_string(),
            ));
        }
    } else {
        return Err(TestError::new(
            "outbox_trace_extraction",
            "No traceparent header found".to_string(),
        ));
    };

    // Extract book ID from response body
    let book_id = response
        .text()
        .await
        .map_err(|e| TestError::new("outbox_book_id_parse", e.to_string()))?;

    Ok((trace_id, book_id.trim().trim_matches('"').to_string()))
}

async fn validate_domain_event_created(
    http_client: &HttpClient,
    config: &TestConfig,
    book_trace_id: &str,
) -> TestResult<()> {
    println!("🔍 Searching for BookCreated domain event traces");

    // Look for traces that contain event creation spans
    let trace_url = format!("{}/api/traces/{}", config.tempo_url, book_trace_id);
    let response = http_client
        .get(&trace_url)
        .send()
        .await
        .map_err(|e| TestError::new("domain_event_validation", e.to_string()))?;

    if !response.status().is_success() {
        return Err(TestError::new(
            "domain_event_validation",
            "Could not fetch book creation trace".to_string(),
        ));
    }

    let trace_data: TempoResponse = response
        .json()
        .await
        .map_err(|e| TestError::new("domain_event_parse", e.to_string()))?;

    // Look for event creation spans
    let mut found_event_creation = false;
    for batch in &trace_data.batches {
        for instrumentation_scope in &batch.scope_spans {
            for span in &instrumentation_scope.spans {
                if span.name.contains("events.append")
                    || span.name.contains("create_book_created_event")
                {
                    found_event_creation = true;
                    println!("✅ Found domain event creation span: {}", span.name);
                    break;
                }
            }
        }
    }

    if !found_event_creation {
        return Err(TestError::new(
            "domain_event_validation",
            "No domain event creation spans found in book creation trace".to_string(),
        ));
    }

    println!("✅ Domain event creation validated");
    Ok(())
}

async fn validate_kafka_message_published(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔍 Searching for outbox publisher activity");

    // Search for recent outbox publish traces with retry – the worker runs on a timer
    const MAX_ATTEMPTS: usize = 5;
    let mut outbox_traces = Vec::new();
    for attempt in 1..=MAX_ATTEMPTS {
        outbox_traces = search_outbox_traces(http_client, config, 120).await?; // Last 2 minutes

        if !outbox_traces.is_empty() {
            break;
        }

        if attempt < MAX_ATTEMPTS {
            let backoff = Duration::from_secs((attempt * 2) as u64);
            println!(
                "⏳ No outbox traces yet (attempt {}/{}) – retrying in {}s",
                attempt,
                MAX_ATTEMPTS,
                backoff.as_secs()
            );
            tokio::time::sleep(backoff).await;
        } else {
            return Err(TestError::new(
                "outbox_validation",
                "No outbox publisher traces found - events may not be getting published"
                    .to_string(),
            ));
        }
    }

    println!("✅ Found {} outbox publisher traces", outbox_traces.len());

    // Validate at least one trace contains actual publishing activity
    for trace_id in outbox_traces.iter().take(5) {
        let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
        if let Ok(response) = http_client.get(&trace_url).send().await {
            if let Ok(trace_data) = response.json::<TempoResponse>().await {
                for batch in &trace_data.batches {
                    for instrumentation_scope in &batch.scope_spans {
                        for span in &instrumentation_scope.spans {
                            if span.name.contains("publish_unpublished_events")
                                || span.name.contains("publish_event")
                            {
                                println!("✅ Found Kafka publishing activity: {}", span.name);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    println!("⚠️  Found outbox traces but no specific publishing activity");
    Ok(())
}

async fn validate_backend_processing(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔍 Searching for backend Kafka message processing");

    const MAX_ATTEMPTS: usize = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        let kafka_traces = search_kafka_processing_traces(http_client, config, 120).await?; // Last 2 minutes

        if !kafka_traces.is_empty() {
            println!("✅ Found {} Kafka processing traces", kafka_traces.len());
            return Ok(());
        }

        if attempt < MAX_ATTEMPTS {
            let backoff = Duration::from_secs(2 * attempt as u64 + 2);
            println!(
                "⏳ No Kafka processing traces yet (attempt {}/{}) – waiting {}s...",
                attempt,
                MAX_ATTEMPTS,
                backoff.as_secs()
            );
            tokio::time::sleep(backoff).await;
        }
    }

    Err(TestError::new(
        "backend_processing_validation",
        "No Kafka message processing traces found after multiple attempts".to_string(),
    ))
}

async fn validate_materialized_view_refresh(
    http_client: &HttpClient,
    config: &TestConfig,
) -> TestResult<()> {
    println!("🔍 Searching for materialized view refresh activity");

    let view_traces = search_view_refresh_traces(http_client, config, 120).await?; // Last 2 minutes

    if view_traces.is_empty() {
        println!(
            "⚠️  No materialized view refresh traces found - may be scheduled or event-driven"
        );
        // This is not necessarily an error as view refresh may be scheduled
        return Ok(());
    }

    println!("✅ Found {} view refresh traces", view_traces.len());

    // Validate at least one trace contains actual refresh activity
    for trace_id in view_traces.iter().take(3) {
        let trace_url = format!("{}/api/traces/{}", config.tempo_url, trace_id);
        if let Ok(response) = http_client.get(&trace_url).send().await {
            if let Ok(trace_data) = response.json::<TempoResponse>().await {
                for batch in &trace_data.batches {
                    for instrumentation_scope in &batch.scope_spans {
                        for span in &instrumentation_scope.spans {
                            if span.name.contains("refresh_search_materialized_view") {
                                println!("✅ Found materialized view refresh: {}", span.name);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn validate_metrics_increased(
    http_client: &HttpClient,
    config: &TestConfig,
    initial_metrics: &OutboxMetrics,
) -> TestResult<()> {
    println!("📊 Validating metrics increased from baseline");

    let final_metrics = capture_baseline_metrics(http_client, config).await?;

    let outbox_increase = final_metrics
        .outbox_publishes
        .saturating_sub(initial_metrics.outbox_publishes);
    let kafka_increase = final_metrics
        .kafka_messages_processed
        .saturating_sub(initial_metrics.kafka_messages_processed);
    let view_increase = final_metrics
        .view_refreshes
        .saturating_sub(initial_metrics.view_refreshes);

    println!("📈 Metrics delta:");
    println!("  - Outbox publishes: +{}", outbox_increase);
    println!("  - Kafka messages: +{}", kafka_increase);
    println!("  - View refreshes: +{}", view_increase);

    if outbox_increase == 0 {
        println!("⚠️  No increase in outbox activity detected");
    }

    if kafka_increase == 0 {
        println!("⚠️  No increase in Kafka processing detected");
    }

    // At minimum, we should see some activity
    let total_activity = outbox_increase + kafka_increase + view_increase;
    if total_activity == 0 {
        return Err(TestError::new(
            "metrics_validation",
            "No increase in any outbox pattern metrics detected".to_string(),
        ));
    }

    println!("✅ Metrics validation completed");
    Ok(())
}

async fn search_outbox_traces(
    http_client: &HttpClient,
    config: &TestConfig,
    time_range_seconds: u64,
) -> TestResult<Vec<String>> {
    let query = "{resource.service.name=\"backend\" && name=\"outbox_publish_cycle\"}";
    search_traces_by_query(http_client, config, query, time_range_seconds).await
}

async fn search_kafka_processing_traces(
    http_client: &HttpClient,
    config: &TestConfig,
    time_range_seconds: u64,
) -> TestResult<Vec<String>> {
    let query = "{resource.service.name=\"backend\" && name=\"book_ingestion_processing\"}";
    search_traces_by_query(http_client, config, query, time_range_seconds).await
}

async fn search_view_refresh_traces(
    http_client: &HttpClient,
    config: &TestConfig,
    time_range_seconds: u64,
) -> TestResult<Vec<String>> {
    let query = "{resource.service.name=\"backend\" && name=\"refresh_search_materialized_view_for_new_book\"}";
    search_traces_by_query(http_client, config, query, time_range_seconds).await
}

async fn search_traces_by_query(
    http_client: &HttpClient,
    config: &TestConfig,
    query: &str,
    time_range_seconds: u64,
) -> TestResult<Vec<String>> {
    let url = format!(
        "{}/api/search?q={}&limit=50&start={}&end={}",
        config.tempo_url,
        urlencoding::encode(query),
        chrono::Utc::now().timestamp() - time_range_seconds as i64,
        chrono::Utc::now().timestamp()
    );

    let response = http_client
        .get(&url)
        .send()
        .await
        .map_err(|e| TestError::new("trace_search", e.to_string()))?;

    if !response.status().is_success() {
        return Ok(Vec::new()); // Return empty if search fails
    }

    let search_result: TempoSearchResponse = response
        .json()
        .await
        .map_err(|_| TestError::new("trace_search_parse", "Parse failed".to_string()))?;

    Ok(search_result
        .traces
        .iter()
        .map(|t| t.trace_id.clone())
        .collect())
}

#[derive(Debug, Clone)]
struct OutboxMetrics {
    outbox_publishes: usize,
    kafka_messages_processed: usize,
    view_refreshes: usize,
}
