use anyhow::{bail, Context, Result};
use preaggregated_metrics::{connect, export, HistogramSnapshot};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_nanos() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .try_into()?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".into());
    match arguments.as_slice() {
        [flag, path] if flag == "--snapshot" => {
            let snapshot: HistogramSnapshot = serde_json::from_slice(
                &std::fs::read(path).context("reading histogram snapshot")?,
            )?;
            snapshot.clone().into_request()?;
            export(&mut connect(endpoint).await?, snapshot).await?;
            println!("Exported snapshot over OTLP/gRPC");
        }
        [flag] if flag == "--demo" => {
            let mut client = connect(endpoint).await?;
            let start_time_unix_nano = now_nanos()?;
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            let mut batches: u64 = 0;
            loop {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => { result?; break; }
                    _ = interval.tick() => {
                        batches += 1;
                        let snapshot = HistogramSnapshot {
                            start_time_unix_nano,
                            time_unix_nano: now_nanos()?,
                            explicit_bounds: vec![0.1, 0.5, 1.0, 2.5],
                            bucket_counts: vec![2 * batches, 5 * batches, 3 * batches, batches, 0],
                            sum: 5.6 * batches as f64,
                        };
                        export(&mut client, snapshot).await?;
                        println!("Exported cumulative snapshot: {} observations", batches * 11);
                    }
                }
            }
        }
        _ => bail!("usage: preaggregated-metrics --demo | --snapshot PATH"),
    }
    Ok(())
}
