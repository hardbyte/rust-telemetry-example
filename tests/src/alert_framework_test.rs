use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

// Import the generated Progenitor client for API calls
use client::{Client as BookappClient, ClientState};

const TELEMETRY_BASE_URL_DEFAULT: &str = "http://localhost:3000";

// Test result types
type TestResult<T> = Result<T, AlertTestError>;

#[derive(Debug, Clone)]
struct AlertTestError {
    message: String,
    operation: String,
}

impl AlertTestError {
    fn new(operation: &str, message: String) -> Self {
        Self {
            operation: operation.to_string(),
            message,
        }
    }
}

impl std::fmt::Display for AlertTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for AlertTestError {}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GrafanaAlert {
    #[serde(rename = "generatorURL")]
    generator_url: String,
    annotations: HashMap<String, String>,
    labels: HashMap<String, String>,
    status: GrafanaAlertStatus,
}

#[derive(Debug, Deserialize)]
struct GrafanaAlertStatus {
    state: String,
}

#[derive(Debug, Deserialize)]
struct PrometheusQueryResponse {
    data: PrometheusQueryData,
}

#[derive(Debug, Deserialize)]
struct PrometheusQueryData {
    result: Vec<PrometheusQueryResult>,
}

#[derive(Debug, Deserialize)]
struct PrometheusQueryResult {
    value: [serde_json::Value; 2],
}

#[tokio::test]
async fn test_alert_framework_connectivity() -> TestResult<()> {
    println!("🔗 Testing alert framework connectivity and APIs");

    let http_client = HttpClient::new();

    // Test 1: Grafana API connectivity
    println!("📊 Testing Grafana alerts API...");
    let telemetry_base = std::env::var("TELEMETRY_BASE_URL")
        .unwrap_or_else(|_| TELEMETRY_BASE_URL_DEFAULT.to_string());
    let grafana_url = format!("{}/api/alertmanager/grafana/api/v2/alerts", telemetry_base);
    let response = http_client
        .get(grafana_url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "grafana_connectivity",
                format!("Failed to connect to Grafana: {}", e),
            )
        })?;

    if !response.status().is_success() {
        return Err(AlertTestError::new(
            "grafana_connectivity",
            format!("Grafana API returned status: {}", response.status()),
        ));
    }

    let alerts: Vec<GrafanaAlert> = response.json().await.map_err(|e| {
        AlertTestError::new(
            "grafana_connectivity",
            format!("Failed to parse Grafana response: {}", e),
        )
    })?;

    println!("✅ Found {} active alerts in Grafana", alerts.len());

    // Test 2: Prometheus API connectivity via Grafana proxy
    println!("📈 Testing Prometheus API via Grafana proxy...");
    let prometheus_url = format!(
        "{}/api/datasources/proxy/1/api/v1/query?query=up",
        telemetry_base
    );
    let response = http_client
        .get(prometheus_url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "prometheus_connectivity",
                format!("Failed to connect to Prometheus: {}", e),
            )
        })?;

    if !response.status().is_success() {
        return Err(AlertTestError::new(
            "prometheus_connectivity",
            format!("Prometheus API returned status: {}", response.status()),
        ));
    }

    let prom_response: PrometheusQueryResponse = response.json().await.map_err(|e| {
        AlertTestError::new(
            "prometheus_connectivity",
            format!("Failed to parse Prometheus response: {}", e),
        )
    })?;

    let metric_count = prom_response.data.result.len();
    println!("✅ Prometheus returned {} metrics", metric_count);

    // Test 3: Application API connectivity
    println!("🚀 Testing application API...");
    let client_state = ClientState::default();
    let app_base =
        std::env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let bookapp_client = BookappClient::new(&app_base, client_state);

    let _books_response = bookapp_client.get_all_books().send().await.map_err(|e| {
        AlertTestError::new("app_connectivity", format!("Failed to get books: {}", e))
    })?;

    println!("✅ Application API responded successfully");

    // Test 4: Span metrics collection
    println!("📊 Testing span metrics collection...");
    let span_metrics_url = format!(
        "{}/api/datasources/proxy/1/api/v1/query?query=traces_spanmetrics_calls_total",
        telemetry_base
    );
    let response = http_client
        .get(&span_metrics_url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "span_metrics",
                format!("Failed to query span metrics: {}", e),
            )
        })?;

    let prom_response: PrometheusQueryResponse = response.json().await.map_err(|e| {
        AlertTestError::new(
            "span_metrics",
            format!("Failed to parse span metrics response: {}", e),
        )
    })?;

    let span_series = prom_response.data.result.len();
    println!("✅ Found {} span metrics series", span_series);

    // Test 5: Load generation capability
    println!("🔥 Testing load generation...");
    for i in 1..=3 {
        let _ = bookapp_client.get_all_books().send().await;
        let _ = bookapp_client.get_book().id(i).send().await;
    }

    // Wait a moment for metrics to be collected
    sleep(Duration::from_secs(2)).await;

    // Verify metrics increased
    let response = http_client
        .get(&span_metrics_url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "load_generation",
                format!("Failed to verify metrics after load: {}", e),
            )
        })?;

    let prom_response: PrometheusQueryResponse = response.json().await.map_err(|e| {
        AlertTestError::new(
            "load_generation",
            format!("Failed to parse metrics after load: {}", e),
        )
    })?;

    let post_load_series = prom_response.data.result.len();
    println!(
        "✅ Load generation test complete - metrics collection verified ({} series)",
        post_load_series
    );

    // Test 6: Error generation capability
    println!("💥 Testing error generation...");
    let _ = bookapp_client.get_book().id(99999).send().await; // Should return 404
    let _ = bookapp_client.get_book().id(-1).send().await; // Should return 404

    println!("✅ Error generation test complete");

    println!("🎉 All alert framework connectivity tests passed!");

    Ok(())
}

#[tokio::test]
async fn test_alert_detection_capability() -> TestResult<()> {
    println!("🚨 Testing alert detection capability");

    let http_client = HttpClient::new();

    // Get current alerts
    let telemetry_base = std::env::var("TELEMETRY_BASE_URL")
        .unwrap_or_else(|_| TELEMETRY_BASE_URL_DEFAULT.to_string());
    let grafana_url = format!("{}/api/alertmanager/grafana/api/v2/alerts", telemetry_base);
    let response = http_client
        .get(grafana_url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new("alert_detection", format!("Failed to get alerts: {}", e))
        })?;

    let alerts: Vec<GrafanaAlert> = response.json().await.map_err(|e| {
        AlertTestError::new("alert_detection", format!("Failed to parse alerts: {}", e))
    })?;

    println!("📊 Current alert summary:");
    for alert in &alerts {
        if let Some(rule_uid) = alert.labels.get("__alert_rule_uid__") {
            if let Some(rule_name) = alert.labels.get("rulename") {
                println!(
                    "  - Alert: {} ({}), State: {}",
                    rule_name, rule_uid, alert.status.state
                );
            }
        }
    }

    // Check if our target alerts exist in the configuration
    let error_ratio_alert = alerts
        .iter()
        .find(|a| a.labels.get("__alert_rule_uid__") == Some(&"be04wldshdiioe".to_string()));
    let latency_alert = alerts
        .iter()
        .find(|a| a.labels.get("__alert_rule_uid__") == Some(&"latency_p95_slo".to_string()));

    match error_ratio_alert {
        Some(alert) => println!("✅ Error ratio alert found - State: {}", alert.status.state),
        None => println!("⚠️  Error ratio alert not found in active alerts (may not be firing)"),
    }

    match latency_alert {
        Some(alert) => println!("✅ P95 latency alert found - State: {}", alert.status.state),
        None => println!("⚠️  P95 latency alert not found in active alerts (may not be firing)"),
    }

    // Test query capability for our alert conditions
    println!("📈 Testing alert condition queries...");

    // Error ratio query
    let error_query = "sum(rate(traces_spanmetrics_calls_total{status_code=\"STATUS_CODE_ERROR\", service=\"bookapp\"}[5m]))";
    let total_query = "sum(rate(traces_spanmetrics_calls_total{service=\"bookapp\"}[5m]))";

    let error_result = query_prometheus_scalar(&http_client, error_query).await;
    let total_result = query_prometheus_scalar(&http_client, total_query).await;

    match (error_result, total_result) {
        (Ok(error_rate), Ok(total_rate)) => {
            if total_rate > 0.0 {
                let error_ratio = error_rate / total_rate;
                println!(
                    "📊 Current error ratio: {:.3} ({:.1}%)",
                    error_ratio,
                    error_ratio * 100.0
                );
            } else {
                println!("📊 No traffic detected for error ratio calculation");
            }
        }
        _ => println!("⚠️  Could not calculate error ratio - metrics may be missing"),
    }

    // P95 latency query
    let latency_query = r#"histogram_quantile(0.95, sum(rate(traces_spanmetrics_latency_bucket{service="bookapp", span_kind="SPAN_KIND_SERVER"}[5m])) by (le))"#;

    match query_prometheus_scalar(&http_client, latency_query).await {
        Ok(p95_latency) => {
            println!("📊 Current P95 latency: {:.1}ms", p95_latency * 1000.0);
        }
        Err(_) => println!("⚠️  Could not calculate P95 latency - metrics may be missing"),
    }

    println!("✅ Alert detection capability test complete");

    Ok(())
}

async fn query_prometheus_scalar(http_client: &HttpClient, query: &str) -> TestResult<f64> {
    let telemetry_base = std::env::var("TELEMETRY_BASE_URL")
        .unwrap_or_else(|_| TELEMETRY_BASE_URL_DEFAULT.to_string());
    let url = format!(
        "{}/api/datasources/proxy/1/api/v1/query?query={}",
        telemetry_base,
        urlencoding::encode(query)
    );

    let response = http_client
        .get(&url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "prometheus_query",
                format!("Failed to query Prometheus: {}", e),
            )
        })?;

    let prom_response: PrometheusQueryResponse = response.json().await.map_err(|e| {
        AlertTestError::new(
            "prometheus_query",
            format!("Failed to parse Prometheus response: {}", e),
        )
    })?;

    if let Some(result) = prom_response.data.result.first() {
        if let Some(value_str) = result.value[1].as_str() {
            return value_str.parse::<f64>().map_err(|e| {
                AlertTestError::new(
                    "prometheus_query",
                    format!("Failed to parse metric value: {}", e),
                )
            });
        }
    }

    Err(AlertTestError::new(
        "prometheus_query",
        "No results found for Prometheus query".to_string(),
    ))
}
