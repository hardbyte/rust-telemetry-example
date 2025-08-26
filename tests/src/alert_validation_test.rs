use reqwest::Client as HttpClient;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::time::{sleep, timeout};

// Import the generated Progenitor client for API calls
use client::{Client as BookappClient, ClientState};

// Configuration constants
const APP_BASE_URL: &str = "http://localhost:8000";
const GRAFANA_BASE_URL: &str = "http://localhost:3000";
const PROMETHEUS_BASE_URL: &str = "http://localhost:3000/api/datasources/proxy/1";
const DEFAULT_TIMEOUT_SECS: u64 = 300;
const ALERT_CHECK_INTERVAL_SECS: u64 = 5;
const ERROR_INJECTION_DURATION_SECS: u64 = 90; // Must be > alert 'for' duration (1m)
const LATENCY_INJECTION_DURATION_SECS: u64 = 360; // Must be > alert 'for' duration (5m)

// Alert configuration from evaluation-group.json
const ERROR_RATIO_ALERT_UID: &str = "be04wldshdiioe";
const LATENCY_P95_ALERT_UID: &str = "latency_p95_slo";
const ERROR_RATIO_THRESHOLD: f64 = 0.05; // 5%
const LATENCY_P95_THRESHOLD_MS: f64 = 500.0; // 500ms

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

/// Test configuration for alert validation
struct AlertTestConfig {
    app_base_url: String,
    grafana_base_url: String,
    prometheus_base_url: String,
    timeout_duration: Duration,
    alert_check_interval: Duration,
}

impl Default for AlertTestConfig {
    fn default() -> Self {
        Self {
            app_base_url: std::env::var("APP_BASE_URL")
                .unwrap_or_else(|_| APP_BASE_URL.to_string()),
            grafana_base_url: std::env::var("GRAFANA_BASE_URL")
                .unwrap_or_else(|_| GRAFANA_BASE_URL.to_string()),
            prometheus_base_url: std::env::var("PROMETHEUS_BASE_URL")
                .unwrap_or_else(|_| PROMETHEUS_BASE_URL.to_string()),
            timeout_duration: Duration::from_secs(
                std::env::var("ALERT_TEST_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(DEFAULT_TIMEOUT_SECS),
            ),
            alert_check_interval: Duration::from_secs(ALERT_CHECK_INTERVAL_SECS),
        }
    }
}

#[tokio::test]
async fn test_error_ratio_alert() -> TestResult<()> {
    let config = AlertTestConfig::default();
    let http_client = HttpClient::new();

    println!(
        "🚨 Testing Error Ratio Alert (threshold: {}%)",
        ERROR_RATIO_THRESHOLD * 100.0
    );

    // Step 1: Verify alert is not firing initially
    verify_alert_not_firing(&http_client, ERROR_RATIO_ALERT_UID, &config).await?;

    // Step 2: Inject errors to trigger the alert
    println!(
        "💥 Injecting errors for {} seconds...",
        ERROR_INJECTION_DURATION_SECS
    );
    let error_injection_task = tokio::spawn({
        let config = config.clone();
        async move { inject_errors(&config, ERROR_INJECTION_DURATION_SECS).await }
    });

    // Step 3: Wait for alert to fire
    println!("⏳ Waiting for error ratio alert to fire...");
    let alert_result = timeout(
        config.timeout_duration,
        wait_for_alert_firing(&http_client, ERROR_RATIO_ALERT_UID, &config),
    )
    .await;

    // Stop error injection
    error_injection_task.abort();

    match alert_result {
        Ok(Ok(_)) => {
            println!("✅ Error ratio alert fired successfully!");

            // Verify the actual error ratio exceeded threshold
            verify_error_ratio_threshold(&http_client, &config).await?;

            // Wait for alert to resolve
            println!("⏳ Waiting for alert to resolve...");
            timeout(
                Duration::from_secs(300), // 5 minutes max wait for resolution
                wait_for_alert_resolved(&http_client, ERROR_RATIO_ALERT_UID, &config),
            )
            .await
            .map_err(|_| {
                AlertTestError::new(
                    "error_ratio_alert",
                    "Alert did not resolve within timeout".to_string(),
                )
            })??;

            println!("✅ Error ratio alert resolved successfully!");
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AlertTestError::new(
            "error_ratio_alert",
            "Alert did not fire within timeout period".to_string(),
        )),
    }
}

#[tokio::test]
async fn test_latency_p95_alert() -> TestResult<()> {
    let config = AlertTestConfig::default();
    let http_client = HttpClient::new();

    println!(
        "🐌 Testing P95 Latency Alert (threshold: {}ms)",
        LATENCY_P95_THRESHOLD_MS
    );

    // Step 1: Verify alert is not firing initially
    verify_alert_not_firing(&http_client, LATENCY_P95_ALERT_UID, &config).await?;

    // Step 2: Inject latency to trigger the alert
    println!(
        "🐌 Injecting latency for {} seconds...",
        LATENCY_INJECTION_DURATION_SECS
    );
    let latency_injection_task = tokio::spawn({
        let config = config.clone();
        async move { inject_latency(&config, LATENCY_INJECTION_DURATION_SECS).await }
    });

    // Step 3: Wait for alert to fire
    println!("⏳ Waiting for P95 latency alert to fire...");
    let alert_result = timeout(
        config.timeout_duration,
        wait_for_alert_firing(&http_client, LATENCY_P95_ALERT_UID, &config),
    )
    .await;

    // Stop latency injection
    latency_injection_task.abort();

    match alert_result {
        Ok(Ok(_)) => {
            println!("✅ P95 latency alert fired successfully!");

            // Verify the actual P95 latency exceeded threshold
            verify_latency_p95_threshold(&http_client, &config).await?;

            // Wait for alert to resolve
            println!("⏳ Waiting for alert to resolve...");
            timeout(
                Duration::from_secs(600), // 10 minutes max wait for resolution
                wait_for_alert_resolved(&http_client, LATENCY_P95_ALERT_UID, &config),
            )
            .await
            .map_err(|_| {
                AlertTestError::new(
                    "latency_p95_alert",
                    "Alert did not resolve within timeout".to_string(),
                )
            })??;

            println!("✅ P95 latency alert resolved successfully!");
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AlertTestError::new(
            "latency_p95_alert",
            "Alert did not fire within timeout period".to_string(),
        )),
    }
}

async fn verify_alert_not_firing(
    http_client: &HttpClient,
    alert_uid: &str,
    config: &AlertTestConfig,
) -> TestResult<()> {
    let alerts = get_grafana_alerts(http_client, config).await?;

    if let Some(alert) = alerts
        .iter()
        .find(|a| a.labels.get("__alert_rule_uid__") == Some(&alert_uid.to_string()))
    {
        if alert.status.state == "active" {
            println!("⚠️  Alert {} is already firing before test started - waiting for it to resolve first...", alert_uid);
            // Wait for the existing alert to resolve
            wait_for_alert_resolved(http_client, alert_uid, config).await?;
            // Wait a bit more to ensure clean state
            sleep(Duration::from_secs(30)).await;
            println!("✅ Alert {} has resolved, proceeding with test", alert_uid);
            return Ok(());
        }
    }

    println!("✅ Verified alert {} is not firing initially", alert_uid);
    Ok(())
}

async fn wait_for_alert_firing(
    http_client: &HttpClient,
    alert_uid: &str,
    config: &AlertTestConfig,
) -> TestResult<()> {
    loop {
        let alerts = get_grafana_alerts(http_client, config).await?;

        if let Some(alert) = alerts
            .iter()
            .find(|a| a.labels.get("__alert_rule_uid__") == Some(&alert_uid.to_string()))
        {
            println!("🔍 Alert {} state: {}", alert_uid, alert.status.state);
            if alert.status.state == "active" {
                return Ok(());
            }
        }

        sleep(config.alert_check_interval).await;
    }
}

async fn wait_for_alert_resolved(
    http_client: &HttpClient,
    alert_uid: &str,
    config: &AlertTestConfig,
) -> TestResult<()> {
    loop {
        let alerts = get_grafana_alerts(http_client, config).await?;

        if let Some(alert) = alerts
            .iter()
            .find(|a| a.labels.get("__alert_rule_uid__") == Some(&alert_uid.to_string()))
        {
            println!("🔍 Alert {} state: {}", alert_uid, alert.status.state);
            if alert.status.state != "active" {
                return Ok(());
            }
        } else {
            // Alert not found in active alerts - it has resolved
            return Ok(());
        }

        sleep(config.alert_check_interval).await;
    }
}

async fn get_grafana_alerts(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<Vec<GrafanaAlert>> {
    let url = format!(
        "{}/api/alertmanager/grafana/api/v2/alerts",
        config.grafana_base_url
    );

    let response = http_client
        .get(&url)
        .basic_auth("admin", Some("admin"))
        .send()
        .await
        .map_err(|e| AlertTestError::new("grafana_api", format!("Failed to get alerts: {}", e)))?;

    if !response.status().is_success() {
        return Err(AlertTestError::new(
            "grafana_api",
            format!("Grafana API returned status: {}", response.status()),
        ));
    }

    let alerts: Vec<GrafanaAlert> = response.json().await.map_err(|e| {
        AlertTestError::new("grafana_api", format!("Failed to parse alerts JSON: {}", e))
    })?;

    Ok(alerts)
}

async fn inject_errors(config: &AlertTestConfig, duration_secs: u64) -> TestResult<()> {
    let client_state = ClientState::default();
    let bookapp_client = BookappClient::new(&config.app_base_url, client_state);

    let end_time = SystemTime::now() + Duration::from_secs(duration_secs);
    let mut request_count = 0;

    while SystemTime::now() < end_time {
        // Make requests that will trigger errors (e.g., invalid book IDs)
        for invalid_id in [99999, -1, 0] {
            let _ = bookapp_client.get_book().id(invalid_id).send().await; // This should return 404/500 errors
            request_count += 1;
        }

        // Also make some successful requests to establish a baseline
        if request_count % 10 == 0 {
            let _ = bookapp_client.get_all_books().send().await;
        }

        sleep(Duration::from_millis(100)).await; // 10 requests per second
    }

    println!(
        "💥 Error injection complete. Made {} error requests",
        request_count
    );
    Ok(())
}

async fn inject_latency(config: &AlertTestConfig, duration_secs: u64) -> TestResult<()> {
    let client_state = ClientState::default();
    let bookapp_client = BookappClient::new(&config.app_base_url, client_state);
    let end_time = SystemTime::now() + Duration::from_secs(duration_secs);
    let mut request_count = 0;

    while SystemTime::now() < end_time {
        // Make many concurrent requests to increase latency through resource contention
        let tasks: Vec<_> = (0..20)
            .map(|_| {
                let client = bookapp_client.clone();
                tokio::spawn(async move {
                    // Make requests that involve database queries to create latency
                    let _ = client.get_all_books().send().await;
                    let _ = client.get_book().id(1).send().await;
                })
            })
            .collect();

        // Wait for all concurrent requests
        for task in tasks {
            let _ = task.await;
        }

        request_count += 20;
        sleep(Duration::from_millis(100)).await; // Brief pause between batches
    }

    println!(
        "🐌 Latency injection complete. Made {} concurrent requests",
        request_count
    );
    Ok(())
}

async fn verify_error_ratio_threshold(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<()> {
    // Query Prometheus to verify actual error ratio
    let error_query = "sum(rate(traces_spanmetrics_calls_total{status_code=\"STATUS_CODE_ERROR\", service=\"bookapp\"}[5m]))";
    let total_query = "sum(rate(traces_spanmetrics_calls_total{service=\"bookapp\"}[5m]))";

    let error_rate = query_prometheus_scalar(http_client, config, error_query).await?;
    let total_rate = query_prometheus_scalar(http_client, config, total_query).await?;

    if total_rate > 0.0 {
        let actual_error_ratio = error_rate / total_rate;
        println!(
            "📊 Actual error ratio: {:.3} (threshold: {:.3})",
            actual_error_ratio, ERROR_RATIO_THRESHOLD
        );

        if actual_error_ratio > ERROR_RATIO_THRESHOLD {
            return Ok(());
        }
    }

    Err(AlertTestError::new(
        "metric_verification",
        "Error ratio did not exceed threshold as expected".to_string(),
    ))
}

async fn verify_latency_p95_threshold(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<()> {
    // Query Prometheus to verify actual P95 latency
    let latency_query = r#"histogram_quantile(0.95, sum(rate(traces_spanmetrics_latency_bucket{service="bookapp", span_kind="SPAN_KIND_SERVER"}[5m])) by (le))"#;

    let actual_p95_latency = query_prometheus_scalar(http_client, config, latency_query).await?;

    println!(
        "📊 Actual P95 latency: {:.1}ms (threshold: {:.1}ms)",
        actual_p95_latency * 1000.0,
        LATENCY_P95_THRESHOLD_MS
    );

    if actual_p95_latency * 1000.0 > LATENCY_P95_THRESHOLD_MS {
        return Ok(());
    }

    Err(AlertTestError::new(
        "metric_verification",
        "P95 latency did not exceed threshold as expected".to_string(),
    ))
}

async fn query_prometheus_scalar(
    http_client: &HttpClient,
    config: &AlertTestConfig,
    query: &str,
) -> TestResult<f64> {
    let url = format!(
        "{}/api/v1/query?query={}",
        config.prometheus_base_url,
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

impl Clone for AlertTestConfig {
    fn clone(&self) -> Self {
        Self {
            app_base_url: self.app_base_url.clone(),
            grafana_base_url: self.grafana_base_url.clone(),
            prometheus_base_url: self.prometheus_base_url.clone(),
            timeout_duration: self.timeout_duration,
            alert_check_interval: self.alert_check_interval,
        }
    }
}
