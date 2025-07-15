// In tests/src/telemetry_test.rs
use opentelemetry::trace::{TraceContextExt, TracerProvider};
use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info_span, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use urlencoding;

use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Registry, EnvFilter};
use tracing_opentelemetry::OpenTelemetryLayer;

static INIT: std::sync::Once = std::sync::Once::new();

fn init_test_tracing() {
    INIT.call_once(|| {
        // Set up OpenTelemetry
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://telemetry:4317".to_string());
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
}

// Response structures for parsing API responses
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

// Helper functions for retry logic and telemetry queries
async fn retry_with_exponential_backoff<F, T, E>(
    operation: F,
    max_attempts: usize,
    base_delay_secs: u64,
    operation_name: &str,
) -> Result<T, String>
where
    F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>> + Send>>,
    E: std::fmt::Debug,
{
    for attempt in 1..=max_attempts {
        println!("Attempt {} for {}", attempt, operation_name);
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                println!("Attempt {} failed for {}: {:?}", attempt, operation_name, e);
                if attempt < max_attempts {
                    let delay = Duration::from_secs(attempt as u64 * base_delay_secs);
                    println!("Waiting {:?} before next attempt...", delay);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
    Err(format!(
        "{} failed after {} attempts",
        operation_name, max_attempts
    ))
}

async fn query_tempo_for_trace(http_client: &HttpClient, trace_id: &str) -> Result<(), String> {
    // Try direct Tempo API first, then Grafana proxy
    let tempo_urls = vec![
        format!("http://telemetry:3200/api/traces/{}", trace_id),
        format!("http://telemetry:3000/api/datasources/proxy/tempo/api/traces/{}", trace_id),
    ];

    for attempt in 1..=10 {
        println!("Attempt {} for Tempo trace query", attempt);
        
        for (i, tempo_url) in tempo_urls.iter().enumerate() {
            println!("Trying URL {}: {}", i + 1, tempo_url);
            
            let response = http_client.get(tempo_url).send().await
                .map_err(|e| format!("Request failed: {:?}", e))?;
            
            let status = response.status();
            println!("Tempo API response status: {}", status);
            
            if status == reqwest::StatusCode::OK {
                let response_text = response.text().await
                    .map_err(|e| format!("Failed to read response: {:?}", e))?;
                println!("Tempo API response body: {}", response_text);
                
                if !response_text.is_empty() && response_text != "{}" 
                    && !response_text.to_lowercase().contains("trace not found") {
                    return Ok(());
                }
            } else if status != reqwest::StatusCode::NOT_FOUND {
                let error_body = response.text().await.unwrap_or_default();
                println!("Error response from {}: {} - {}", tempo_url, status, error_body);
            }
        }
        
        if attempt < 10 {
            let delay = Duration::from_secs(attempt as u64 * 3);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }
    
    Err(format!("Tempo trace query failed after 10 attempts for trace {}", trace_id))
}

async fn query_loki_for_logs(http_client: &HttpClient, trace_id: &str) -> Result<(), String> {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos();
    let start_ns = now_ns - (300 * 1_000_000_000);

    let log_query = format!("{{trace_id=\\\"{}\\\"}}", trace_id);
    let loki_query_url = format!(
        "http://telemetry:3000/loki/api/v1/query_range?query={}&start={}&end={}&direction=forward",
        urlencoding::encode(&log_query),
        start_ns,
        now_ns
    );

    println!("Loki query URL: {}", loki_query_url);

    for attempt in 1..=10 {
        println!("Attempt {} for Loki logs query", attempt);
        
        let response = http_client.get(&loki_query_url).send().await
            .map_err(|e| format!("Request failed: {:?}", e))?;
        
        let status = response.status();
        println!("Loki API response status: {}", status);
        
        if status == reqwest::StatusCode::OK {
            let response_text = response.text().await
                .map_err(|e| format!("Failed to read response: {:?}", e))?;
            
            let loki_response: LokiResponse = serde_json::from_str(&response_text)
                .map_err(|e| format!("Failed to parse JSON: {:?}", e))?;
            
            let log_count: usize = loki_response.data.result.iter()
                .map(|stream| stream.values.len()).sum();
            
            if log_count > 0 {
                println!("Found {} log entries in Loki for trace ID {}.", log_count, trace_id);
                return Ok(());
            }
        }
        
        if attempt < 10 {
            let delay = Duration::from_secs(attempt as u64 * 3);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }
    
    Err(format!("Loki logs query failed after 10 attempts for trace {}", trace_id))
}

async fn query_prometheus_for_metrics(
    http_client: &HttpClient,
    trace_id: &str,
) -> Result<(), String> {
    let prom_query = format!(
        "sum(traces_spanmetrics_calls_total{{service=\"bookapp\", span_kind=\"server\", span_name=\"HTTP GET /books\", trace_id=\"{}\"}}) by (span_name)",
        trace_id
    );

    let prometheus_query_url = format!(
        "http://telemetry:3000/api/datasources/proxy/prometheus/api/v1/query?query={}",
        urlencoding::encode(&prom_query)
    );

    println!("Prometheus query URL: {}", prometheus_query_url);

    for attempt in 1..=7 {
        println!("Attempt {} for Prometheus metrics query", attempt);
        
        let response = http_client.get(&prometheus_query_url).send().await
            .map_err(|e| format!("Request failed: {:?}", e))?;
        
        let status = response.status();
        println!("Prometheus API response status: {}", status);
        
        if status == reqwest::StatusCode::OK {
            let response_text = response.text().await
                .map_err(|e| format!("Failed to read response: {:?}", e))?;
            
            let prom_response: PrometheusResponse = serde_json::from_str(&response_text)
                .map_err(|e| format!("Failed to parse JSON: {:?}", e))?;
            
            if prom_response.status == "success" && !prom_response.data.result.is_empty() {
                if let Some(first_result) = prom_response.data.result.first() {
                    if let Some(value_str) = first_result.value.get(1).and_then(|v| v.as_str()) {
                        let val: f64 = value_str.parse()
                            .map_err(|e| format!("Failed to parse value '{}': {:?}", value_str, e))?;
                        
                        if val >= 1.0 {
                            println!("Successfully found metric with value {} >= 1.0", val);
                            return Ok(());
                        }
                    }
                }
            }
        }
        
        if attempt < 7 {
            let delay = Duration::from_secs(attempt as u64 * 3);
            println!("Waiting {:?} before next attempt...", delay);
            tokio::time::sleep(delay).await;
        }
    }
    
    Err(format!("Prometheus metrics query failed after 7 attempts for trace {}", trace_id))
}

#[tokio::test]
async fn test_root_endpoint_generates_telemetry() {
    init_test_tracing();
    
    let (trace_id, http_client) = execute_traced_request().await;
    wait_for_trace_propagation().await;
    verify_telemetry_in_all_systems(&http_client, &trace_id).await;
    
    println!("✅ Test completed successfully!");
}

async fn execute_traced_request() -> (String, HttpClient) {
    let root_span = info_span!("test_request_to_books_endpoint");
    let _guard = root_span.enter();
    
    let trace_id = extract_trace_id(&root_span);
    let http_client = HttpClient::new();
    let request = build_traced_request(&http_client, &root_span).await;
    
    println!("📡 Sending traced request to /books endpoint (trace: {})", trace_id);
    let response = http_client.execute(request).await.expect("Request failed");
    
    assert!(response.status().is_success(), "Request to /books endpoint failed with status: {}", response.status());
    println!("✅ Request successful ({})", response.status());
    
    (trace_id, http_client)
}

fn extract_trace_id(span: &Span) -> String {
    let otel_context = span.context();
    let span_ref = otel_context.span();
    let span_context = span_ref.span_context();
    let trace_id = format!("{:032x}", span_context.trace_id());
    println!("🔍 Generated trace ID: {} (length: {})", trace_id, trace_id.len());
    trace_id
}

async fn build_traced_request(http_client: &HttpClient, span: &Span) -> reqwest::Request {
    let mut request = http_client
        .get("http://app:8000/books")
        .build()
        .expect("Failed to build request");
    
    let otel_context = span.context();
    opentelemetry::global::get_text_map_propagator(|injector| {
        injector.inject_context(&otel_context, &mut helpers::RequestCarrier::new(&mut request));
    });
    
    request
}

async fn wait_for_trace_propagation() {
    println!("⏳ Waiting for trace propagation...");
    tokio::time::sleep(Duration::from_secs(10)).await;
}

async fn verify_telemetry_in_all_systems(http_client: &HttpClient, trace_id: &str) {
    println!("🔎 Verifying telemetry data in all systems...");
    
    verify_tempo_trace(http_client, trace_id).await;
    verify_loki_logs(http_client, trace_id).await;
    verify_prometheus_metrics(http_client, trace_id).await;
}

async fn verify_tempo_trace(http_client: &HttpClient, trace_id: &str) {
    println!("🎯 Querying Tempo for trace: {}", trace_id);
    query_tempo_for_trace(http_client, trace_id)
        .await
        .unwrap_or_else(|e| panic!("❌ Tempo verification failed: {}", e));
    println!("✅ Tempo verification successful");
}

async fn verify_loki_logs(http_client: &HttpClient, trace_id: &str) {
    println!("📋 Querying Loki for logs with trace: {}", trace_id);
    query_loki_for_logs(http_client, trace_id)
        .await
        .unwrap_or_else(|e| panic!("❌ Loki verification failed: {}", e));
    println!("✅ Loki verification successful");
}

async fn verify_prometheus_metrics(http_client: &HttpClient, trace_id: &str) {
    println!("📊 Querying Prometheus for metrics with trace: {}", trace_id);
    query_prometheus_for_metrics(http_client, trace_id)
        .await
        .unwrap_or_else(|e| panic!("❌ Prometheus verification failed: {}", e));
    println!("✅ Prometheus verification successful");
}

mod helpers {
    use reqwest::header::{HeaderName, HeaderValue};
    use reqwest::Request;
    use std::{str::FromStr, collections::HashMap};

    pub struct RequestCarrier<'a> {
        request: &'a mut Request,
    }

    impl<'a> RequestCarrier<'a> {
        pub fn new(request: &'a mut Request) -> Self {
            RequestCarrier { request }
        }
    }

    impl<'a> opentelemetry::propagation::Injector for RequestCarrier<'a> {
        fn set(&mut self, key: &str, value: String) {
            let header_name = HeaderName::from_str(key).expect("Must be header name");
            let header_value = HeaderValue::from_str(&value).expect("Must be a header value");
            self.request.headers_mut().insert(header_name, header_value);
        }
    }

    pub struct HeaderMapCarrier<'a> {
        headers: &'a mut HashMap<String, String>,
    }

    impl<'a> HeaderMapCarrier<'a> {
        pub fn new(headers: &'a mut HashMap<String, String>) -> Self {
            HeaderMapCarrier { headers }
        }
    }

    impl<'a> opentelemetry::propagation::Injector for HeaderMapCarrier<'a> {
        fn set(&mut self, key: &str, value: String) {
            self.headers.insert(key.to_string(), value);
        }
    }
}
