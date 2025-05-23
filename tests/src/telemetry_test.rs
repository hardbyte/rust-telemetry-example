// In tests/src/telemetry_test.rs
use reqwest::Client as HttpClient; // Aliased to avoid conflict
use tracing::info_span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use opentelemetry::trace::{Tracer, TraceContextExt, SpanKind, TraceId}; // Added TraceId
use opentelemetry::global;
use serde_json::Value; // Added for Loki response parsing
use urlencoding; // Added for URL encoding Loki query

// Reference the init function from lib.rs
use integration_tests::{init_test_tracing_provider, flush_traces};


#[tokio::test]
async fn test_root_endpoint_generates_telemetry() {
    println!("Initializing test tracing provider...");
    init_test_tracing_provider(); // Initialize tracing for this test
    println!("Test tracing provider initialized.");

    let tracer = global::tracer("integration-tester.rust-test");

    println!("Creating root span...");
    let root_span = tracer.span_builder("test_request_to_root_endpoint")
        .with_kind(SpanKind::Client)
        .start(&tracer);
    println!("Root span created: {:?}", root_span.context().span_id());

    let trace_id = root_span.context().trace_id().to_hex();
    println!("Trace ID for current test: {}", trace_id);
    
    let cx = root_span.context();

    let http_client = HttpClient::new();
    let mut request = http_client.get("http://app:8000/")
        .build()
        .expect("Failed to build request");

    opentelemetry::global::get_text_map_propagator(|injector| {
        injector.inject_context(&cx, &mut helpers::RequestCarrier::new(&mut request));
    });
    
    println!("Sending request to http://app:8000/ with trace context: {:?}", request.headers().get("traceparent"));

    let response = http_client.execute(request).await.expect("Request failed");
    
    println!("Response Status: {}", response.status());
    assert!(response.status().is_success(), "Request to / endpoint failed");

    println!("Ending root span...");
    root_span.end(); 
    println!("Root span ended.");

    println!("Flushing traces...");
    flush_traces(); // Call flush
    println!("Traces flushed. Sleeping to allow propagation...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    println!("Querying Tempo for trace ID: {}", trace_id);
    let tempo_query_url = format!(
        "http://telemetry:3000/api/datasources/proxy/tempo/api/traces/{}",
        trace_id
    );
    
    // Reuse http_client if already in scope and suitable, or create a new one
    // let http_client = HttpClient::new(); 
    
    // Retry mechanism for Tempo query
    let mut attempts = 0;
    let max_attempts = 5;
    let mut trace_found = false;
    while attempts < max_attempts {
        attempts += 1;
        println!("Attempt {} to query Tempo: {}", attempts, tempo_query_url);
        match http_client.get(&tempo_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Tempo API response status: {}", status);
                if status == reqwest::StatusCode::OK {
                    let response_text = response.text().await.unwrap_or_default();
                    println!("Tempo API response body: {}", response_text);
                    // Basic check: if OK and body is not empty or a known "not found" message.
                    // Tempo might return an empty JSON `{{}}` or other content for a found trace.
                    // A more robust check would be to parse the JSON and verify span details.
                    if !response_text.is_empty() && response_text != "{}" && !response_text.to_lowercase().contains("trace not found") {
                        trace_found = true;
                        break;
                    } else {
                        println!("Trace {} not found in Tempo yet, or empty response. Body: {}", trace_id, response_text);
                    }
                } else if status == reqwest::StatusCode::NOT_FOUND {
                    println!("Trace {} not found in Tempo (404).", trace_id);
                } else {
                    let error_body = response.text().await.unwrap_or_default();
                    println!("Tempo API returned non-OK status: {}. Body: {}", status, error_body);
                }
            }
            Err(e) => {
                println!("Error querying Tempo API: {:?}", e);
            }
        }
        if attempts < max_attempts {
            println!("Waiting before next Tempo query attempt...");
            tokio::time::sleep(tokio::time::Duration::from_secs( (attempts * 2) as u64 )).await; // Exponential backoff
        }
    }

    assert!(trace_found, "Trace {} was not found in Tempo after {} attempts", trace_id, max_attempts);
    println!("Successfully verified trace {} in Tempo.", trace_id);

    // Start of Loki query section
    println!("Querying Loki for logs with trace ID: {}", trace_id); 
    
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_nanos();
    // Query for logs in the last 5 minutes (300 seconds)
    let start_ns = now_ns - (300 * 1_000_000_000); 

    let log_query = format!("{{trace_id=\\\"{}\\\"}}", trace_id); // Escaped quotes for LogQL

    let loki_query_url = format!(
        "http://telemetry:3000/loki/api/v1/query_range?query={}&start={}&end={}&direction=forward",
        urlencoding::encode(&log_query), 
        start_ns,
        now_ns
    );

    println!("Loki query URL: {}", loki_query_url); 

    let mut attempts = 0;
    let max_attempts_loki = 5; // Using a different variable name for clarity
    let mut logs_found = false;
    while attempts < max_attempts_loki {
        attempts += 1;
        println!("Attempt {} to query Loki: {}", attempts, loki_query_url); 
        match http_client.get(&loki_query_url).send().await { 
            Ok(response) => {
                let status = response.status();
                println!("Loki API response status: {}", status); 
                if status == reqwest::StatusCode::OK {
                    let response_text = response.text().await.unwrap_or_default();
                    println!("Loki API response body (raw): {}", response_text); 
                    
                    match serde_json::from_str::<serde_json::Value>(&response_text) { 
                        Ok(json_body) => {
                            if let Some(data) = json_body.get("data") {
                                if let Some(result) = data.get("result") {
                                    if let Some(streams) = result.as_array() {
                                        if !streams.is_empty() {
                                            let mut actual_log_entries = 0;
                                            for stream in streams {
                                                if let Some(values) = stream.get("values") {
                                                    if let Some(entries) = values.as_array() {
                                                        actual_log_entries += entries.len();
                                                    }
                                                }
                                            }
                                            if actual_log_entries > 0 {
                                                logs_found = true;
                                                println!("Found {} log entries in Loki for trace ID {}.", actual_log_entries, trace_id); 
                                                break; 
                                            } else {
                                                println!("Loki response indicates streams but no actual log entries for trace ID {}.", trace_id); 
                                            }
                                        } else {
                                            println!("No log streams found in Loki for trace ID {}.", trace_id); 
                                        }
                                    } else {
                                        println!("Loki response 'result' is not an array.");
                                    }
                                } else {
                                    println!("Loki response missing 'result' field in 'data'.");
                                }
                            } else {
                                println!("Loki response missing 'data' field.");
                            }
                        }
                        Err(parse_err) => {
                            println!("Failed to parse Loki JSON response: {:?}. Body: {}", parse_err, response_text); 
                        }
                    }
                } else {
                    let error_body = response.text().await.unwrap_or_default();
                    println!("Loki API returned non-OK status: {}. Body: {}", status, error_body); 
                }
            }
            Err(e) => {
                println!("Error querying Loki API: {:?}", e); 
            }
        }
        if !logs_found && attempts < max_attempts_loki { // only sleep if logs not found and more attempts left
            println!("Waiting before next Loki query attempt...");
            tokio::time::sleep(tokio::time::Duration::from_secs( (attempts * 2) as u64 )).await; 
        }
    }

    assert!(logs_found, "Logs for trace ID {} were not found in Loki after {} attempts", trace_id, max_attempts_loki); 
    println!("Successfully verified logs for trace ID {} in Loki.", trace_id); 
    // End of Loki query section

    // Start of Prometheus/Mimir query section
    println!("Querying Prometheus/Mimir for metrics related to the trace ID: {}", trace_id);

    // PromQL query: Check for server-side spans for the 'bookapp' service, route '/', kind SERVER.
    // The span name for Axum routes is typically "HTTP {method} {route}".
    // We expect a span named "HTTP GET /" for the main request.
    // traces_spanmetrics_calls_total is a counter for calls.
    let prom_query = format!(
        "sum(traces_spanmetrics_calls_total{{service=\"bookapp\", span_kind=\"server\", span_name=\"HTTP GET /\", trace_id=\"{}\"}}) by (span_name)",
        trace_id 
    );
    // Alternatively, without trace_id if it's not a reliable label on metrics immediately:
    // let prom_query = "sum(rate(traces_spanmetrics_calls_total{service=\"bookapp\", span_kind=\"server\", span_name=\"HTTP GET /\"}[1m])) > 0".to_string();

    let prometheus_query_url = format!(
        "http://telemetry:3000/api/datasources/proxy/prometheus/api/v1/query?query={}",
        urlencoding::encode(&prom_query)
    );

    println!("Prometheus query URL: {}", prometheus_query_url);

    let mut attempts = 0;
    let max_attempts_prometheus = 7; // Increased attempts as metrics can take longer
    let mut metrics_found_and_valid = false;
    while attempts < max_attempts_prometheus {
        attempts += 1;
        println!("Attempt {} to query Prometheus: {}", attempts, prometheus_query_url);
        match http_client.get(&prometheus_query_url).send().await {
            Ok(response) => {
                let status = response.status();
                println!("Prometheus API response status: {}", status);
                if status == reqwest::StatusCode::OK {
                    let response_text = response.text().await.unwrap_or_default();
                    println!("Prometheus API response body (raw): {}", response_text);
                    
                    match serde_json::from_str::<serde_json::Value>(&response_text) {
                        Ok(json_body) => {
                            if json_body.get("status").and_then(|s| s.as_str()) == Some("success") {
                                if let Some(data) = json_body.get("data") {
                                    if let Some(result) = data.get("result") {
                                        if let Some(result_array) = result.as_array() {
                                            if !result_array.is_empty() {
                                                // We expect one result from the sum by span_name
                                                if let Some(metric_item) = result_array.get(0) {
                                                    if let Some(value_array) = metric_item.get("value") {
                                                        if value_array.len() == 2 { // [timestamp, value_str]
                                                            if let Some(value_str) = value_array[1].as_str() {
                                                                match value_str.parse::<f64>() {
                                                                    Ok(val) => {
                                                                        if val >= 1.0 { // Expect at least 1 call
                                                                            metrics_found_and_valid = true;
                                                                            println!("Successfully found metric {} with value {} >= 1.0", prom_query, val);
                                                                            break;
                                                                        } else {
                                                                            println!("Metric value {} is < 1.0", val);
                                                                        }
                                                                    }
                                                                    Err(e) => println!("Failed to parse metric value string '{}': {:?}", value_str, e),
                                                                }
                                                            } else {
                                                                println!("Metric value is not a string.");
                                                            }
                                                        } else {
                                                            println!("Metric value array does not have 2 elements.");
                                                        }
                                                    } else {
                                                        println!("Metric item missing 'value' field.");
                                                    }
                                                } else {
                                                     println!("Result array is empty, but expected one item.");
                                                }
                                            } else {
                                                println!("No metrics found in Prometheus for query: {}. Result array is empty.", prom_query);
                                            }
                                        } else {
                                            println!("Prometheus query result is not an array.");
                                        }
                                    } else {
                                        println!("Prometheus query response missing 'result' field in 'data'.");
                                    }
                                } else {
                                    println!("Prometheus query response missing 'data' field.");
                                }
                            } else {
                                 println!("Prometheus query status was not 'success'. Full body: {}", response_text);
                            }
                        }
                        Err(parse_err) => {
                            println!("Failed to parse Prometheus JSON response: {:?}. Body: {}", parse_err, response_text);
                        }
                    }
                } else {
                    let error_body = response.text().await.unwrap_or_default();
                    println!("Prometheus API returned non-OK status: {}. Body: {}", status, error_body);
                }
            }
            Err(e) => {
                println!("Error querying Prometheus API: {:?}", e);
            }
        }
        if !metrics_found_and_valid && attempts < max_attempts_prometheus {
            println!("Waiting before next Prometheus query attempt...");
            tokio::time::sleep(tokio::time::Duration::from_secs( (attempts * 3) as u64 )).await; // Longer backoff for metrics
        }
    }

    assert!(metrics_found_and_valid, "Metrics for query '{}' were not found or not valid in Prometheus after {} attempts", prom_query, max_attempts_prometheus);
    println!("Successfully verified metrics in Prometheus for query: {}", prom_query);
    // End of Prometheus/Mimir query section

    println!("Test finished.");
}

mod helpers {
    use reqwest::Request;
    use std::str::FromStr;
    use reqwest::header::{HeaderName, HeaderValue};

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
}
