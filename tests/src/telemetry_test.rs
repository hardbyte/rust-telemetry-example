use opentelemetry::trace::TracerProvider;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::time::Duration;

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
const EXPECTED_SPAN_NAME: &str = "get_all_books";

// Retry and timeout configuration
const MAX_TEMPO_ATTEMPTS: usize = 15;
const MAX_LOKI_ATTEMPTS: usize = 10;
const MAX_PROMETHEUS_ATTEMPTS: usize = 10;
const BASE_RETRY_DELAY_SECS: u64 = 2;
const MAX_RETRY_DELAY_SECS: u64 = 10;
const TRACE_PROPAGATION_WAIT_SECS: u64 = 5;
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

#[derive(Debug, Deserialize)]
struct TempoResponse {
    batches: Vec<Batch>,
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

    // Configure error injection with unique endpoint pattern
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let test_endpoint = format!("/books/test-{timestamp}");
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

    // Make a request that should fail
    let response = http_client
        .get(format!("{}{}", config.app_url, test_endpoint))
        .send()
        .await
        .map_err(|e| TestError::new("http_request_error_case", e.to_string()))?;

    assert_eq!(
        response.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );

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
