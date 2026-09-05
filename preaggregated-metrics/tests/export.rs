use opentelemetry_proto::tonic::{
    collector::metrics::v1::{
        metrics_service_server::{MetricsService, MetricsServiceServer},
        ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse,
    },
    metrics::v1::{metric::Data, AggregationTemporality},
};
use preaggregated_metrics::{connect, export, HistogramSnapshot};
use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

fn snapshot() -> HistogramSnapshot {
    HistogramSnapshot {
        start_time_unix_nano: 1_700_000_000_000_000_000,
        time_unix_nano: 1_700_000_060_000_000_000,
        explicit_bounds: vec![0.1, 0.5, 1.0, 2.5],
        bucket_counts: vec![2, 5, 3, 1, 0],
        sum: 5.6,
    }
}

#[test]
fn rejects_invalid_snapshots() {
    let mut cases = vec![];
    let mut invalid = snapshot();
    invalid.explicit_bounds.swap(0, 1);
    cases.push(invalid);
    let mut invalid = snapshot();
    invalid.bucket_counts.pop();
    cases.push(invalid);
    let mut invalid = snapshot();
    invalid.bucket_counts[0] = u64::MAX;
    cases.push(invalid);
    let mut invalid = snapshot();
    invalid.time_unix_nano = invalid.start_time_unix_nano;
    cases.push(invalid);
    let mut invalid = snapshot();
    invalid.sum = f64::NAN;
    cases.push(invalid);
    for invalid in cases {
        assert!(invalid.into_request().is_err());
    }
}

#[derive(Clone, Default)]
struct Receiver {
    requests: Arc<Mutex<Vec<ExportMetricsServiceRequest>>>,
    reject: bool,
}

#[tonic::async_trait]
impl MetricsService for Receiver {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        self.requests.lock().unwrap().push(request.into_inner());
        Ok(Response::new(ExportMetricsServiceResponse {
            partial_success: self.reject.then(|| ExportMetricsPartialSuccess {
                rejected_data_points: 1,
                error_message: "test rejection".into(),
            }),
        }))
    }
}

#[tokio::test]
async fn exports_exact_buckets_and_preserves_start_across_connections() {
    let receiver = Receiver::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let service = MetricsServiceServer::new(receiver.clone());
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    for batch in 1..=2 {
        let mut next = snapshot();
        next.bucket_counts
            .iter_mut()
            .for_each(|count| *count *= batch);
        next.sum *= batch as f64;
        next.time_unix_nano += batch * 1_000_000_000;
        let mut client = connect(endpoint.clone()).await.unwrap();
        export(&mut client, next).await.unwrap();
    }
    let requests = receiver.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for (index, request) in requests.iter().enumerate() {
        let metric = &request.resource_metrics[0].scope_metrics[0].metrics[0];
        assert_eq!(metric.unit, "s");
        let Some(Data::Histogram(histogram)) = &metric.data else {
            panic!("expected histogram")
        };
        assert_eq!(
            histogram.aggregation_temporality,
            AggregationTemporality::Cumulative as i32
        );
        let point = &histogram.data_points[0];
        let batch = index as u64 + 1;
        assert_eq!(point.start_time_unix_nano, snapshot().start_time_unix_nano);
        assert_eq!(
            point.bucket_counts,
            vec![2 * batch, 5 * batch, 3 * batch, batch, 0]
        );
        assert_eq!(point.explicit_bounds, vec![0.1, 0.5, 1.0, 2.5]);
        assert_eq!(point.count, 11 * batch);
        assert_eq!(point.sum, Some(5.6 * batch as f64));
    }
    server.abort();
}

#[tokio::test]
async fn partial_rejection_is_not_reported_as_success() {
    let receiver = Receiver {
        reject: true,
        ..Default::default()
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(MetricsServiceServer::new(receiver))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let error = export(&mut connect(endpoint).await.unwrap(), snapshot())
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("collector rejected 1 data points"));
    server.abort();
}
