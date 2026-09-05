use anyhow::{ensure, Result};
use opentelemetry_proto::tonic::{
    collector::metrics::v1::{
        metrics_service_client::MetricsServiceClient, ExportMetricsServiceRequest,
    },
    common::v1::{any_value::Value, AnyValue, InstrumentationScope, KeyValue},
    metrics::v1::{
        metric::Data, AggregationTemporality, Histogram, HistogramDataPoint, Metric,
        ResourceMetrics, ScopeMetrics,
    },
    resource::v1::Resource,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HistogramSnapshot {
    pub start_time_unix_nano: u64,
    pub time_unix_nano: u64,
    pub explicit_bounds: Vec<f64>,
    pub bucket_counts: Vec<u64>,
    pub sum: f64,
}

impl HistogramSnapshot {
    pub fn into_request(self) -> Result<ExportMetricsServiceRequest> {
        ensure!(
            self.start_time_unix_nano > 0 && self.time_unix_nano > self.start_time_unix_nano,
            "snapshot end must be after a nonzero aggregation start"
        );
        ensure!(
            self.explicit_bounds
                .iter()
                .all(|bound| bound.is_finite() && *bound >= 0.0)
                && self
                    .explicit_bounds
                    .windows(2)
                    .all(|bounds| bounds[0] < bounds[1]),
            "duration bounds must be finite, nonnegative and strictly increasing"
        );
        ensure!(
            self.bucket_counts.len() == self.explicit_bounds.len() + 1,
            "OTLP needs one bucket population per bound, plus the overflow bucket"
        );
        ensure!(
            self.sum.is_finite() && self.sum >= 0.0,
            "duration sum must be finite and nonnegative"
        );
        let count = self
            .bucket_counts
            .iter()
            .try_fold(0u64, |total, count| total.checked_add(*count))
            .ok_or_else(|| anyhow::anyhow!("bucket counts overflow u64"))?;
        ensure!(
            count != 0 || self.sum == 0.0,
            "an empty histogram must have a zero sum"
        );
        let point = HistogramDataPoint {
            start_time_unix_nano: self.start_time_unix_nano,
            time_unix_nano: self.time_unix_nano,
            count,
            sum: Some(self.sum),
            explicit_bounds: self.explicit_bounds,
            bucket_counts: self.bucket_counts,
            ..Default::default()
        };
        Ok(ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(Resource {
                    attributes: vec![KeyValue {
                        key: "service.name".into(),
                        value: Some(AnyValue {
                            value: Some(Value::StringValue("histogram-example".into())),
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                scope_metrics: vec![ScopeMetrics {
                    scope: Some(InstrumentationScope {
                        name: "preaggregated-metrics".into(),
                        version: env!("CARGO_PKG_VERSION").into(),
                        ..Default::default()
                    }),
                    metrics: vec![Metric {
                        name: "example.processing.duration".into(),
                        description: "Processing durations from an aggregated snapshot.".into(),
                        unit: "s".into(),
                        data: Some(Data::Histogram(Histogram {
                            aggregation_temporality: AggregationTemporality::Cumulative.into(),
                            data_points: vec![point],
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        })
    }
}

pub async fn connect(endpoint: String) -> Result<MetricsServiceClient<Channel>> {
    let channel = Endpoint::from_shared(endpoint)?
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .connect()
        .await?;
    Ok(MetricsServiceClient::new(channel))
}

pub async fn export(
    client: &mut MetricsServiceClient<Channel>,
    snapshot: HistogramSnapshot,
) -> Result<()> {
    let response = client.export(snapshot.into_request()?).await?.into_inner();
    if let Some(partial) = response.partial_success {
        ensure!(
            partial.rejected_data_points == 0,
            "collector rejected {} data points: {}",
            partial.rejected_data_points,
            partial.error_message
        );
        if !partial.error_message.is_empty() {
            eprintln!("Collector warning: {}", partial.error_message);
        }
    }
    Ok(())
}
