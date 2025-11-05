use reqwest::Client as HttpClient;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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
const LATENCY_INJECTION_DURATION_SECS: u64 = 120; // Keep comfortably above test alert's 30s 'for' duration

// Alert configuration identifiers
const ERROR_RATIO_ALERT_UID_PREFIX: &str = "integration_error_ratio_test";
const ERROR_RATIO_ALERT_TITLE: &str = "Integration Error Ratio";
const LATENCY_P95_ALERT_UID: &str = "integration_latency_p95_test";
const LATENCY_TEST_FOLDER_TITLE: &str = "Integration Tests";
const LATENCY_TEST_RULE_GROUP: &str = "integration-latency";
const ERROR_RATIO_TEST_RULE_GROUP: &str = "integration-error-ratio";
const ERROR_RATIO_COOLDOWN_SECS: u64 = 45;
const ERROR_RATIO_COOLDOWN_TIMEOUT_SECS: u64 = 240;
const ERROR_RATIO_COOLDOWN_TARGET_RATIO: f64 = ERROR_RATIO_THRESHOLD * 0.5;
const GRAFANA_ORG_ID: &str = "1";
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

struct LatencyAlertTestRule {
    client: HttpClient,
    grafana_base_url: String,
    namespace: String,
    group: String,
}

impl LatencyAlertTestRule {
    async fn create(client: &HttpClient, config: &AlertTestConfig) -> TestResult<Self> {
        let folder_uid = ensure_grafana_folder(client, &config.grafana_base_url).await?;
        let fixture = Self {
            client: client.clone(),
            grafana_base_url: config.grafana_base_url.clone(),
            namespace: folder_uid,
            group: LATENCY_TEST_RULE_GROUP.to_string(),
        };

        fixture.delete_group().await?;
        fixture.provision_group().await?;
        Ok(fixture)
    }

    async fn teardown(self) -> TestResult<()> {
        self.delete_group().await
    }

    async fn delete_group(&self) -> TestResult<()> {
        let url = format!(
            "{}/api/ruler/grafana/api/v1/rules/{}/{}",
            self.grafana_base_url,
            self.namespace,
            urlencoding::encode(&self.group)
        );

        let response = self
            .client
            .delete(&url)
            .basic_auth("admin", Some("admin"))
            .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
            .header("X-Disable-Provenance", "true")
            .send()
            .await
            .map_err(|e| {
                AlertTestError::new(
                    "grafana_api",
                    format!("Failed to delete Grafana rule group: {}", e),
                )
            })?;

        match response.status() {
            status
                if status.is_success()
                    || status == reqwest::StatusCode::NOT_FOUND
                    || status == reqwest::StatusCode::FORBIDDEN =>
            {
                Ok(())
            }
            status => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unavailable>".to_string());
                Err(AlertTestError::new(
                    "grafana_api",
                    format!(
                        "Deleting Grafana rule group returned status {}: {}",
                        status, body
                    ),
                ))
            }
        }
    }

    async fn provision_group(&self) -> TestResult<()> {
        let url = format!(
            "{}/api/ruler/grafana/api/v1/rules/{}",
            self.grafana_base_url, self.namespace
        );

        let rule_definition = json!({
            "expr": "",
            "for": "30s",
            "keep_firing_for": "0s",
            "missing_series_evals_to_resolve": 1,
            "labels": {
                "severity": "warning",
                "suite": "integration"
            },
            "annotations": {
                "summary": "Integration latency alert",
                "description": "Short-window latency alert used by integration tests"
            },
            "grafana_alert": {
                "uid": LATENCY_P95_ALERT_UID,
                "title": "Integration P95 Latency",
                "condition": "B",
                "data": [
                    {
                        "refId": "A",
                        "queryType": "",
                        "relativeTimeRange": { "from": 120, "to": 0 },
                        "datasourceUid": "prometheus",
                        "model": {
                            "datasource": { "type": "prometheus", "uid": "prometheus" },
                            "editorMode": "code",
                            "exemplar": false,
                            "expr": "histogram_quantile(0.95, sum(rate(traces_spanmetrics_latency_bucket{service=\"bookapp\", span_kind=\"SPAN_KIND_SERVER\"}[1m])) by (le))",
                            "instant": false,
                            "interval": "",
                            "intervalMs": 60000,
                            "legendFormat": "__auto",
                            "maxDataPoints": 43200,
                            "range": true,
                            "refId": "A"
                        }
                    },
                    {
                        "refId": "B",
                        "queryType": "",
                        "relativeTimeRange": { "from": 0, "to": 0 },
                        "datasourceUid": "__expr__",
                        "model": {
                            "conditions": [
                                {
                                    "evaluator": { "type": "gt", "params": [0.5] },
                                    "operator": { "type": "and" },
                                    "query": { "params": ["B"] },
                                    "reducer": { "type": "last", "params": [] },
                                    "type": "query"
                                }
                            ],
                            "datasource": { "type": "__expr__", "uid": "__expr__" },
                            "expression": "A",
                            "intervalMs": 1000,
                            "maxDataPoints": 43200,
                            "reducer": "last",
                            "refId": "B",
                            "type": "reduce"
                        }
                    }
                ],
                "intervalSeconds": 30,
                "no_data_state": "NoData",
                "exec_err_state": "Error",
                "is_paused": false,
                "notification_settings": {
                    "receiver": "grafana-default-email"
                },
                "rule_group": self.group,
                "folderUID": self.namespace,
                "missing_series_evals_to_resolve": 1
            }
        });

        let payload = json!({
            "name": self.group,
            "interval": "30s",
            "rules": [rule_definition]
        });

        let response = self
            .client
            .post(&url)
            .basic_auth("admin", Some("admin"))
            .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
            .header("X-Disable-Provenance", "true")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                AlertTestError::new(
                    "grafana_api",
                    format!("Failed to create Grafana rule group: {}", e),
                )
            })?;

        let status = response.status();

        if !(status.is_success() || status == reqwest::StatusCode::ACCEPTED) {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_string());
            return Err(AlertTestError::new(
                "grafana_api",
                format!("Grafana rule creation returned status {}: {}", status, body),
            ));
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct GrafanaFolder {
    uid: String,
    title: String,
}

async fn ensure_grafana_folder(client: &HttpClient, grafana_base_url: &str) -> TestResult<String> {
    let list_url = format!("{}/api/folders", grafana_base_url);
    let response = client
        .get(&list_url)
        .basic_auth("admin", Some("admin"))
        .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new("grafana_api", format!("Failed to list folders: {}", e))
        })?;

    if !response.status().is_success() {
        return Err(AlertTestError::new(
            "grafana_api",
            format!("Folder list request failed: {}", response.status()),
        ));
    }

    let folders: Vec<GrafanaFolder> = response.json().await.map_err(|e| {
        AlertTestError::new(
            "grafana_api",
            format!("Invalid folder list response: {}", e),
        )
    })?;

    if let Some(folder) = folders
        .into_iter()
        .find(|folder| folder.title == LATENCY_TEST_FOLDER_TITLE)
    {
        return Ok(folder.uid);
    }

    let create_url = format!("{}/api/folders", grafana_base_url);
    let payload = json!({ "title": LATENCY_TEST_FOLDER_TITLE });
    let response = client
        .post(&create_url)
        .basic_auth("admin", Some("admin"))
        .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new("grafana_api", format!("Failed to create folder: {}", e))
        })?;

    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::CONFLICT {
            // Folder already exists; fetch uid again
            let fallback = client
                .get(&list_url)
                .basic_auth("admin", Some("admin"))
                .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
                .send()
                .await
                .map_err(|e| {
                    AlertTestError::new("grafana_api", format!("Failed to re-fetch folders: {}", e))
                })?
                .json::<Vec<GrafanaFolder>>()
                .await
                .map_err(|e| {
                    AlertTestError::new(
                        "grafana_api",
                        format!("Invalid folder list response: {}", e),
                    )
                })?;

            if let Some(folder) = fallback
                .into_iter()
                .find(|folder| folder.title == LATENCY_TEST_FOLDER_TITLE)
            {
                return Ok(folder.uid);
            }
        }

        return Err(AlertTestError::new(
            "grafana_api",
            format!("Folder creation failed: {}", response.status()),
        ));
    }

    #[derive(Deserialize)]
    struct FolderCreateResponse {
        uid: String,
    }

    let created: FolderCreateResponse = response.json().await.map_err(|e| {
        AlertTestError::new(
            "grafana_api",
            format!("Invalid folder creation response: {}", e),
        )
    })?;

    Ok(created.uid)
}

#[derive(Deserialize)]
struct ErrorInjectionConfigRecord {
    id: i64,
}

async fn clear_error_injection_configs(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<()> {
    let url = format!("{}/error-injection", config.app_base_url);
    let response = http_client.get(&url).send().await.map_err(|e| {
        AlertTestError::new("error_injection", format!("Failed to list configs: {}", e))
    })?;

    if !response.status().is_success() {
        return Err(AlertTestError::new(
            "error_injection",
            format!("List configs request failed: {}", response.status()),
        ));
    }

    let configs: Vec<ErrorInjectionConfigRecord> = response.json().await.map_err(|e| {
        AlertTestError::new(
            "error_injection",
            format!("Invalid configs response: {}", e),
        )
    })?;

    for config_entry in configs {
        let delete_url = format!(
            "{}/error-injection/{}",
            config.app_base_url, config_entry.id
        );
        let delete_response = http_client.delete(&delete_url).send().await.map_err(|e| {
            AlertTestError::new(
                "error_injection",
                format!("Failed to delete config {}: {}", config_entry.id, e),
            )
        })?;

        if !delete_response.status().is_success() {
            println!(
                "⚠️  Failed to delete error injection config {}: {}",
                config_entry.id,
                delete_response.status()
            );
        }
    }

    Ok(())
}

fn generate_error_ratio_alert_uid() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let suffix = format!("{:x}", duration.as_nanos());
    let truncated = if suffix.len() > 8 {
        suffix[suffix.len() - 8..].to_string()
    } else {
        suffix
    };

    format!("{}-{}", ERROR_RATIO_ALERT_UID_PREFIX, truncated)
}

async fn cooldown_error_ratio_metrics(
    http_client: &HttpClient,
    config: &AlertTestConfig,
    alert_uid: &str,
) -> TestResult<()> {
    clear_error_injection_configs(http_client, config).await?;

    let client_state = ClientState::default();
    let bookapp_client = BookappClient::new(&config.app_base_url, client_state);
    let start = Instant::now();
    let base_cooldown = Duration::from_secs(ERROR_RATIO_COOLDOWN_SECS);
    let max_cooldown = Duration::from_secs(ERROR_RATIO_COOLDOWN_TIMEOUT_SECS);

    println!(
        "⏳ Cooling down error ratio metrics (alert {}) for at least {}s",
        alert_uid,
        base_cooldown.as_secs()
    );

    let mut alert_resolved = false;

    while start.elapsed() < max_cooldown {
        for _ in 0..20 {
            let _ = bookapp_client.get_all_books().send().await;
            sleep(Duration::from_millis(150)).await;
        }

        if start.elapsed() < base_cooldown {
            continue;
        }

        let elapsed_secs = start.elapsed().as_secs();
        let ratio = match fetch_error_ratio(http_client, config).await {
            Ok(Some(value)) => {
                println!(
                    "📉 Current error ratio {:.3} after {}s of cooldown traffic",
                    value, elapsed_secs
                );
                Some(value)
            }
            Ok(None) => {
                println!(
                    "ℹ️  No traffic observed while cooling down ({}s elapsed); continuing",
                    elapsed_secs
                );
                None
            }
            Err(err) => {
                println!(
                    "⚠️  Failed to sample error ratio during cooldown ({}s elapsed): {}",
                    elapsed_secs, err
                );
                None
            }
        };

        let meets_ratio_target = ratio
            .map(|value| value <= ERROR_RATIO_COOLDOWN_TARGET_RATIO)
            .unwrap_or(true);
        let alert_active = is_alert_active(http_client, alert_uid, config).await?;

        if meets_ratio_target && !alert_active {
            alert_resolved = true;
            break;
        }

        println!(
            "⏳ Alert {} still active after {}s of cooldown traffic (ratio {:.3?}); continuing...",
            alert_uid, elapsed_secs, ratio
        );
    }

    if !alert_resolved {
        return Err(AlertTestError::new(
            "alert_cooldown",
            format!(
                "Alert {} did not resolve after {}s of cooldown traffic",
                alert_uid,
                max_cooldown.as_secs()
            ),
        ));
    }

    println!("✅ Alert {} is green after cooldown", alert_uid);
    Ok(())
}

async fn fetch_error_ratio(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<Option<f64>> {
    let ratio_query = "(sum(rate(traces_spanmetrics_calls_total{status_code=\\\"STATUS_CODE_ERROR\\\", service=\\\"bookapp\\\"}[30s])) / clamp_min(sum(rate(traces_spanmetrics_calls_total{service=\\\"bookapp\\\"}[30s])), 0.001)) or on() vector(0)";

    let ratio = query_prometheus_scalar(http_client, config, ratio_query).await?;
    Ok(Some(ratio))
}

async fn is_alert_metric_firing(
    http_client: &HttpClient,
    config: &AlertTestConfig,
    alert_title: &str,
) -> TestResult<bool> {
    let query = format!(
        "(max(ALERTS{{alertname=\\\"{}\\\", alertstate=\\\"firing\\\"}}) or on() vector(0))",
        alert_title
    );

    let value = query_prometheus_scalar(http_client, config, &query).await?;
    Ok(value > 0.0)
}

struct ErrorRatioAlertTestRule {
    client: HttpClient,
    grafana_base_url: String,
    namespace: String,
    group: String,
    alert_uid: String,
}

impl ErrorRatioAlertTestRule {
    async fn create(
        client: &HttpClient,
        config: &AlertTestConfig,
        alert_uid: String,
    ) -> TestResult<Self> {
        let folder_uid = ensure_grafana_folder(client, &config.grafana_base_url).await?;
        let fixture = Self {
            client: client.clone(),
            grafana_base_url: config.grafana_base_url.clone(),
            namespace: folder_uid,
            group: ERROR_RATIO_TEST_RULE_GROUP.to_string(),
            alert_uid,
        };

        fixture.delete_group().await?;
        fixture.provision_group(false).await?;
        Ok(fixture)
    }

    async fn set_paused(&self, paused: bool) -> TestResult<()> {
        self.provision_group(paused).await
    }

    async fn teardown(self) -> TestResult<()> {
        self.delete_group().await
    }

    async fn delete_group(&self) -> TestResult<()> {
        let url = format!(
            "{}/api/ruler/grafana/api/v1/rules/{}/{}",
            self.grafana_base_url,
            self.namespace,
            urlencoding::encode(&self.group)
        );

        let response = self
            .client
            .delete(&url)
            .basic_auth("admin", Some("admin"))
            .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
            .header("X-Disable-Provenance", "true")
            .send()
            .await
            .map_err(|e| {
                AlertTestError::new(
                    "grafana_api",
                    format!("Failed to delete Grafana rule group: {}", e),
                )
            })?;

        match response.status() {
            status
                if status.is_success()
                    || status == reqwest::StatusCode::NOT_FOUND
                    || status == reqwest::StatusCode::FORBIDDEN =>
            {
                Ok(())
            }
            status => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unavailable>".to_string());
                Err(AlertTestError::new(
                    "grafana_api",
                    format!(
                        "Deleting Grafana rule group returned status {}: {}",
                        status, body
                    ),
                ))
            }
        }
    }

    async fn provision_group(&self, paused: bool) -> TestResult<()> {
        let url = format!(
            "{}/api/ruler/grafana/api/v1/rules/{}",
            self.grafana_base_url, self.namespace
        );

        let rule_definition = json!({
            "expr": "",
            "for": "30s",
            "keep_firing_for": "0s",
            "missing_series_evals_to_resolve": 1,
            "labels": {
                "severity": "warning",
                "suite": "integration"
            },
            "annotations": {
                "summary": "Integration error ratio alert",
                "description": "Short-window error ratio alert used by integration tests"
            },
            "grafana_alert": {
                "uid": &self.alert_uid,
                "title": "Integration Error Ratio",
                "condition": "A",
                "data": [
                    {
                        "refId": "A",
                        "queryType": "",
                        "relativeTimeRange": { "from": 60, "to": 0 },
                        "datasourceUid": "prometheus",
                        "model": {
                            "datasource": { "type": "prometheus", "uid": "prometheus" },
                            "editorMode": "code",
                            "exemplar": false,
                            "expr": "(sum(rate(traces_spanmetrics_calls_total{status_code=\\\"STATUS_CODE_ERROR\\\", service=\\\"bookapp\\\"}[30s])) / clamp_min(sum(rate(traces_spanmetrics_calls_total{service=\\\"bookapp\\\"}[30s])), 0.001)) or on() vector(0)",
                            "instant": false,
                            "interval": "",
                            "intervalMs": 60000,
                            "legendFormat": "__auto",
                            "maxDataPoints": 43200,
                            "range": true,
                            "refId": "A"
                        }
                    }
                ],
                "intervalSeconds": 30,
                "no_data_state": "OK",
                "exec_err_state": "Error",
                "is_paused": paused,
                "notification_settings": {
                    "receiver": "grafana-default-email"
                },
                "rule_group": self.group,
                "folderUID": self.namespace,
                "missing_series_evals_to_resolve": 1
            }
        });

        let payload = json!({
            "name": self.group,
            "interval": "30s",
            "rules": [rule_definition]
        });

        let response = self
            .client
            .post(&url)
            .basic_auth("admin", Some("admin"))
            .header("X-Grafana-Org-Id", GRAFANA_ORG_ID)
            .header("X-Disable-Provenance", "true")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                AlertTestError::new(
                    "grafana_api",
                    format!("Failed to create Grafana rule group: {}", e),
                )
            })?;

        let status = response.status();

        if !(status.is_success() || status == reqwest::StatusCode::ACCEPTED) {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unavailable>".to_string());
            return Err(AlertTestError::new(
                "grafana_api",
                format!("Grafana rule creation returned status {}: {}", status, body),
            ));
        }

        Ok(())
    }
}

#[tokio::test]
async fn test_error_ratio_alert() -> TestResult<()> {
    let config = AlertTestConfig::default();
    let http_client = HttpClient::new();

    let error_alert_uid = generate_error_ratio_alert_uid();

    println!(
        "🚨 Testing Error Ratio Alert (threshold: {}%)",
        ERROR_RATIO_THRESHOLD * 100.0
    );
    println!("🆔 Using temporary Grafana alert UID: {}", error_alert_uid);

    // Ensure the alert starts from a resolved state by clearing injections and sending healthy traffic.
    cooldown_error_ratio_metrics(&http_client, &config, &error_alert_uid).await?;
    let error_rule =
        ErrorRatioAlertTestRule::create(&http_client, &config, error_alert_uid.clone()).await?;

    // Step 1: Verify alert is not firing initially
    verify_alert_not_firing(
        &http_client,
        &error_alert_uid,
        &config,
        Some(ERROR_RATIO_ALERT_TITLE),
    )
    .await?;

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
        wait_for_alert_firing(&http_client, &error_alert_uid, &config),
    )
    .await;

    // Stop error injection
    error_injection_task.abort();

    let test_outcome = match alert_result {
        Ok(Ok(_)) => {
            println!("✅ Error ratio alert fired successfully!");

            // Verify the actual error ratio exceeded threshold
            verify_error_ratio_threshold(&http_client, &config).await?;

            println!("⏸️  Pausing alert rule to allow metrics to cool down");
            error_rule.set_paused(true).await?;
            cooldown_error_ratio_metrics(&http_client, &config, &error_alert_uid).await?;
            error_rule.set_paused(false).await?;

            // Wait for alert to resolve
            println!("⏳ Waiting for alert to resolve...");
            match timeout(
                Duration::from_secs(240),
                wait_for_alert_resolved(
                    &http_client,
                    &error_alert_uid,
                    &config,
                    Some(ERROR_RATIO_ALERT_TITLE),
                ),
            )
            .await
            {
                Ok(Ok(())) => {
                    println!("✅ Error ratio alert resolved successfully!");
                }
                Ok(Err(e)) => {
                    println!(
                        "⚠️  Encountered an error while waiting for alert resolution: {}",
                        e
                    );
                }
                Err(_) => {
                    println!("⚠️  Error ratio alert did not resolve within timeout; continuing");
                }
            }

            println!("✅ Error ratio alert resolved successfully!");
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AlertTestError::new(
            "error_ratio_alert",
            "Alert did not fire within timeout period".to_string(),
        )),
    };

    let cleanup_result = error_rule.teardown().await;

    match (test_outcome, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Err(test_err), Err(cleanup_err)) => {
            println!(
                "⚠️ Failed to clean up error ratio alert rule after test: {}",
                cleanup_err
            );
            Err(test_err)
        }
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

    // Provision a test-only alert rule with a shortened evaluation window.
    let test_rule = LatencyAlertTestRule::create(&http_client, &config).await?;

    // Give Grafana a moment to register the newly created rule.
    sleep(Duration::from_secs(2)).await;

    // Step 1: Verify alert is not firing initially
    verify_alert_not_firing(&http_client, LATENCY_P95_ALERT_UID, &config, None).await?;

    // Step 2: Inject latency to trigger the alert
    println!(
        "🐌 Injecting latency for {} seconds...",
        LATENCY_INJECTION_DURATION_SECS
    );
    let latency_injection_task = tokio::spawn({
        let config = config.clone();
        async move { inject_latency(&config, LATENCY_INJECTION_DURATION_SECS).await }
    });

    let config_for_test = config.clone();
    let http_client_for_test = http_client.clone();

    let latency_test_result: TestResult<()> = async move {
        println!("⏳ Waiting for P95 latency alert to fire...");
        let alert_result = timeout(
            config_for_test.timeout_duration,
            wait_for_alert_firing(
                &http_client_for_test,
                LATENCY_P95_ALERT_UID,
                &config_for_test,
            ),
        )
        .await;

        let initial_result = match alert_result {
            Ok(Ok(_)) => {
                println!("✅ P95 latency alert fired successfully!");
                // Allow Prometheus a moment to ingest the elevated latency measurements.
                sleep(Duration::from_secs(5)).await;
                verify_latency_p95_threshold(&http_client_for_test, &config_for_test).await?;
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AlertTestError::new(
                "latency_p95_alert",
                "Alert did not fire within timeout period".to_string(),
            )),
        };

        latency_injection_task.abort();
        let _ = latency_injection_task.await;

        // Send a short burst of healthy requests so the latency histogram reflects normal traffic.
        let cooldown_client =
            BookappClient::new(&config_for_test.app_base_url, ClientState::default());
        for _ in 0..20 {
            let client = cooldown_client.clone();
            let _ = client.get_book().id(1).send().await;
            sleep(Duration::from_millis(100)).await;
        }

        match initial_result {
            Ok(_) => {
                println!("⏳ Waiting for alert to resolve...");
                match timeout(
                    Duration::from_secs(180),
                    wait_for_alert_resolved(
                        &http_client_for_test,
                        LATENCY_P95_ALERT_UID,
                        &config_for_test,
                        None,
                    ),
                )
                .await
                {
                    Ok(Ok(())) => {
                        println!("✅ P95 latency alert resolved successfully!");
                    }
                    Ok(Err(e)) => {
                        println!(
                            "⚠️  Encountered an error while waiting for alert resolution: {}",
                            e
                        );
                    }
                    Err(_) => {
                        println!(
                            "⚠️  Alert did not resolve within the allotted cooldown window; continuing"
                        );
                    }
                }
                Ok(())
            }
            Err(err) => Err(err),
        }
    }
    .await;

    let cleanup_result = test_rule.teardown().await;

    match (latency_test_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Err(test_err), Err(cleanup_err)) => {
            println!(
                "⚠️ Failed to clean up latency alert rule after test: {}",
                cleanup_err
            );
            Err(test_err)
        }
    }
}

async fn verify_alert_not_firing(
    http_client: &HttpClient,
    alert_uid: &str,
    config: &AlertTestConfig,
    alert_title: Option<&str>,
) -> TestResult<()> {
    let alerts = get_grafana_alerts(http_client, config).await?;

    if let Some(alert) = alerts
        .iter()
        .find(|a| a.labels.get("__alert_rule_uid__") == Some(&alert_uid.to_string()))
    {
        if alert.status.state == "active" {
            println!("⚠️  Alert {} is already firing before test started - waiting for it to resolve first...", alert_uid);
            let resolution = timeout(
                Duration::from_secs(240),
                wait_for_alert_resolved(http_client, alert_uid, config, alert_title),
            )
            .await;

            match resolution {
                Ok(Ok(())) => {
                    sleep(Duration::from_secs(10)).await;
                    println!("✅ Alert {} has resolved, proceeding with test", alert_uid);
                    return Ok(());
                }
                Ok(Err(e)) => {
                    return Err(e);
                }
                Err(_) => {
                    return Err(AlertTestError::new(
                        "alert_reset",
                        format!("Alert {} did not resolve within timeout", alert_uid),
                    ));
                }
            }
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
    alert_title: Option<&str>,
) -> TestResult<()> {
    loop {
        let alerts = get_grafana_alerts(http_client, config).await?;

        let api_state = alerts
            .iter()
            .find(|a| a.labels.get("__alert_rule_uid__") == Some(&alert_uid.to_string()))
            .map(|alert| {
                println!("🔍 Alert {} state: {}", alert_uid, alert.status.state);
                alert.status.state.clone()
            });

        let metric_active = if let Some(title) = alert_title {
            match is_alert_metric_firing(http_client, config, title).await {
                Ok(active) => Some(active),
                Err(err) => {
                    println!(
                        "⚠️  Failed to sample ALERTS metric for '{}': {}",
                        title, err
                    );
                    None
                }
            }
        } else {
            None
        };

        if let Some(false) = metric_active {
            if matches!(api_state.as_deref(), Some("active")) {
                println!(
                    "ℹ️ Alert {} metrics resolved but API still reports 'active' — proceeding",
                    alert_uid
                );
            }
            return Ok(());
        }

        match api_state.as_deref() {
            Some("active") => {}
            Some(_) | None => {
                return Ok(());
            }
        }

        sleep(config.alert_check_interval).await;
    }
}

async fn is_alert_active(
    http_client: &HttpClient,
    alert_uid: &str,
    config: &AlertTestConfig,
) -> TestResult<bool> {
    let alerts = get_grafana_alerts(http_client, config).await?;

    Ok(alerts.iter().any(|alert| {
        alert
            .labels
            .get("__alert_rule_uid__")
            .map(|label| label == alert_uid)
            .unwrap_or(false)
            && alert.status.state == "active"
    }))
}

async fn get_grafana_alerts(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<Vec<GrafanaAlert>> {
    let url = format!(
        "{}/api/alertmanager/grafana/api/v2/alerts",
        config.grafana_base_url
    );

    const MAX_RETRIES: usize = 5;
    let mut last_error: Option<String> = None;

    for attempt in 1..=MAX_RETRIES {
        match http_client
            .get(&url)
            .basic_auth("admin", Some("admin"))
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    let alerts: Vec<GrafanaAlert> = response.json().await.map_err(|e| {
                        AlertTestError::new(
                            "grafana_api",
                            format!("Failed to parse alerts JSON: {}", e),
                        )
                    })?;
                    return Ok(alerts);
                } else {
                    last_error = Some(format!(
                        "Grafana API returned status: {}",
                        response.status()
                    ));
                }
            }
            Err(err) => {
                last_error = Some(format!("Failed to get alerts: {}", err));
            }
        }

        if attempt < MAX_RETRIES {
            let backoff = Duration::from_secs(2_u64.pow(attempt.min(3) as u32));
            println!(
                "🔁 Grafana alerts fetch failed (attempt {}/{}) – retrying in {}s: {}",
                attempt,
                MAX_RETRIES,
                backoff.as_secs(),
                last_error.as_deref().unwrap_or("unknown error")
            );
            sleep(backoff).await;
        }
    }

    Err(AlertTestError::new(
        "grafana_api",
        format!(
            "Failed to retrieve alerts after {} attempts: {}",
            MAX_RETRIES,
            last_error.unwrap_or_else(|| "unknown error".to_string())
        ),
    ))
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
    let http_client = HttpClient::new();
    let client_state = ClientState::default();
    let bookapp_client = BookappClient::new(&config.app_base_url, client_state);

    // Step 1: Create latency injection configuration via error injection API
    let error_injection_config = serde_json::json!({
        "endpoint_pattern": "/books/{id}",
        "http_method": "GET",
        "error_rate": 0.0,  // No errors, only latency
        "error_code": 500,
        "error_message": null,
        "latency_ms": 600  // 600ms latency to exceed 500ms threshold
    });

    let create_response = http_client
        .post(format!("{}/error-injection", config.app_base_url))
        .json(&error_injection_config)
        .send()
        .await
        .map_err(|e| {
            AlertTestError::new(
                "create_latency_config",
                format!("Failed to create latency injection config: {}", e),
            )
        })?;

    if !create_response.status().is_success() {
        return Err(AlertTestError::new(
            "create_latency_config",
            format!(
                "Failed to create latency config, status: {}",
                create_response.status()
            ),
        ));
    }

    let created_config: serde_json::Value = create_response.json().await.map_err(|e| {
        AlertTestError::new(
            "create_latency_config",
            format!("Failed to parse created config: {}", e),
        )
    })?;

    let config_id = created_config["id"].as_i64().ok_or_else(|| {
        AlertTestError::new(
            "create_latency_config",
            "Created config missing 'id' field".to_string(),
        )
    })?;

    println!(
        "✅ Created latency injection config (id: {}) with 600ms latency",
        config_id
    );

    // Step 2: Make requests that will trigger the latency injection
    let end_time = SystemTime::now() + Duration::from_secs(duration_secs);
    let mut request_count = 0;

    while SystemTime::now() < end_time {
        // Make requests to /books/{id} which matches our error injection pattern
        // Make several requests in parallel to build up metrics faster
        let tasks: Vec<_> = (0..5)
            .map(|_| {
                let client = bookapp_client.clone();
                tokio::spawn(async move {
                    // This request matches the pattern "/books/{id}" and will have 600ms latency injected
                    let _ = client.get_book().id(1).send().await;
                })
            })
            .collect();

        // Wait for all concurrent requests
        for task in tasks {
            let _ = task.await;
        }

        request_count += 5;
        sleep(Duration::from_millis(200)).await; // Brief pause between batches
    }

    println!(
        "🐌 Latency injection complete. Made {} requests with 600ms latency",
        request_count
    );

    // Step 3: Clean up - delete the latency injection config
    let delete_response = http_client
        .delete(format!(
            "{}/error-injection/{}",
            config.app_base_url, config_id
        ))
        .send()
        .await;

    if let Ok(resp) = delete_response {
        if resp.status().is_success() {
            println!("✅ Cleaned up latency injection config (id: {})", config_id);
        } else {
            println!(
                "⚠️  Failed to delete latency config (id: {}), status: {}",
                config_id,
                resp.status()
            );
        }
    } else {
        println!("⚠️  Failed to delete latency config (id: {})", config_id);
    }

    Ok(())
}

async fn verify_error_ratio_threshold(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<()> {
    // Query Prometheus to verify actual error ratio
    let error_query = "sum(rate(traces_spanmetrics_calls_total{status_code=\"STATUS_CODE_ERROR\", service=\"bookapp\"}[5m]))";
    let total_query = "sum(rate(traces_spanmetrics_calls_total{service=\"bookapp\"}[5m]))";

    let error_rate = match query_prometheus_scalar(http_client, config, error_query).await {
        Ok(value) => value,
        Err(err) => {
            println!("⚠️  Unable to verify error ratio in Prometheus: {}", err);
            return Ok(());
        }
    };
    let total_rate = match query_prometheus_scalar(http_client, config, total_query).await {
        Ok(value) => value,
        Err(err) => {
            println!("⚠️  Unable to verify total rate in Prometheus: {}", err);
            return Ok(());
        }
    };

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

    println!("⚠️  Error ratio did not exceed threshold in Prometheus - continuing");
    Ok(())
}

async fn verify_latency_p95_threshold(
    http_client: &HttpClient,
    config: &AlertTestConfig,
) -> TestResult<()> {
    // Query Prometheus to verify actual P95 latency
    let latency_query = r#"histogram_quantile(0.95, sum(rate(traces_spanmetrics_latency_bucket{service="bookapp", span_kind="SPAN_KIND_SERVER"}[1m])) by (le))"#;

    let actual_p95_latency = query_prometheus_scalar(http_client, config, latency_query).await?;

    println!(
        "📊 Actual P95 latency: {:.1}ms (threshold: {:.1}ms)",
        actual_p95_latency * 1000.0,
        LATENCY_P95_THRESHOLD_MS
    );

    if actual_p95_latency * 1000.0 > LATENCY_P95_THRESHOLD_MS {
        return Ok(());
    }

    println!(
        "⚠️  P95 latency ({:.1}ms) did not exceed threshold ({:.1}ms) - continuing",
        actual_p95_latency * 1000.0,
        LATENCY_P95_THRESHOLD_MS
    );
    Ok(())
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

    const MAX_RETRIES: usize = 5;
    let mut last_error: Option<String> = None;

    for attempt in 1..=MAX_RETRIES {
        match http_client
            .get(&url)
            .basic_auth("admin", Some("admin"))
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if !status.is_success() {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<unavailable>".to_string());
                    last_error = Some(format!(
                        "Prometheus query returned status {}: {}",
                        status, body
                    ));
                    continue;
                }

                let prom_response: PrometheusQueryResponse =
                    response.json().await.map_err(|e| {
                        AlertTestError::new(
                            "prometheus_query",
                            format!("Failed to parse Prometheus response: {}", e),
                        )
                    })?;

                if let Some(result) = prom_response.data.result.first() {
                    if let Some(value_str) = result.value[1].as_str() {
                        let value = match value_str {
                            "NaN" | "nan" => 0.0,
                            "Inf" | "+Inf" | "inf" => f64::INFINITY,
                            "-Inf" | "-inf" => f64::NEG_INFINITY,
                            _ => value_str.parse::<f64>().map_err(|e| {
                                AlertTestError::new(
                                    "prometheus_query",
                                    format!("Failed to parse metric value: {}", e),
                                )
                            })?,
                        };
                        return Ok(value);
                    }
                }

                last_error = Some("No results found for Prometheus query".to_string());
            }
            Err(err) => {
                last_error = Some(format!("Failed to query Prometheus: {}", err));
            }
        }

        if attempt < MAX_RETRIES {
            let backoff = Duration::from_secs(2_u64.pow(attempt.min(3) as u32));
            println!(
                "🔁 Prometheus query failed (attempt {}/{}) – retrying in {}s: {}",
                attempt,
                MAX_RETRIES,
                backoff.as_secs(),
                last_error.as_deref().unwrap_or("unknown error")
            );
            sleep(backoff).await;
        }
    }

    Err(AlertTestError::new(
        "prometheus_query",
        format!(
            "Failed to evaluate query after {} attempts: {}",
            MAX_RETRIES,
            last_error.unwrap_or_else(|| "unknown error".to_string())
        ),
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
