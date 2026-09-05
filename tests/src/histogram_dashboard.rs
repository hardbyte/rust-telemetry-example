use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[tokio::test]
async fn grafana_dashboard_queries_show_exported_histogram(
) -> Result<(), Box<dyn std::error::Error>> {
    let base =
        std::env::var("TELEMETRY_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let dashboard: Value = client
        .get(format!(
            "{base}/api/dashboards/uid/preaggregated-histograms"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let panels = dashboard["dashboard"]["panels"]
        .as_array()
        .ok_or("missing panels")?;
    let mut verified = 0;
    for panel in panels {
        let Some(expression) = panel["targets"][0]["expr"].as_str() else {
            continue;
        };
        let mut observed = None;
        for _ in 0..30 {
            let response: Value = client
                .get(format!(
                    "{base}/api/datasources/proxy/uid/prometheus/api/v1/query"
                ))
                .query(&[("query", expression)])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            assert_eq!(response["status"], "success", "{response}");
            observed = response["data"]["result"][0]["value"][1]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value > 0.0);
            if observed
                .is_some_and(|value| panel["id"].as_u64() != Some(2) || (value - 2.2).abs() < 0.5)
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let value = observed.ok_or_else(|| format!("no finite positive data for {expression}"))?;
        match panel["id"].as_u64() {
            Some(1) => assert!(value >= 11.0),
            Some(2) => assert!((value - 2.2).abs() < 0.5, "rate: {value}"),
            Some(3) => assert!((value - 5.6 / 11.0).abs() < 0.001, "mean: {value}"),
            Some(4) => assert!((value - 1.675).abs() < 0.001, "p95: {value}"),
            _ => panic!("unexpected query panel"),
        }
        println!("{}: {value}", panel["title"]);
        verified += 1;
    }
    assert_eq!(verified, 4);
    Ok(())
}
