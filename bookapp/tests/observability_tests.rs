#[cfg(test)]
mod observability_tests {
    #![allow(clippy::module_inception)]
    #![allow(dead_code)]

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Extension,
    };
    use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
    use bookapp::book_details::{BookDetailsProvider, StubBookDetailsProvider};
    use bookapp::database::DatabasePools;
    use bookapp_dal::PgPool;
    use dotenv::dotenv;
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry::{global, KeyValue};
    use opentelemetry_sdk::{
        metrics::{
            data::{AggregatedMetrics, MetricData},
            InMemoryMetricExporter, PeriodicReader, SdkMeterProvider,
        },
        propagation::TraceContextPropagator,
        trace::{self, InMemorySpanExporter, Sampler, SdkTracerProvider},
        Resource,
    };
    use rdkafka::producer::FutureProducer;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tower::ServiceExt;
    use tracing_subscriber::{layer::SubscriberExt, Layer, Registry};

    // --- Log Capture Infrastructure ---

    #[derive(Clone)]
    struct LogCaptureLayer {
        logs: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl<S> Layer<S> for LogCaptureLayer
    where
        S: tracing::Subscriber,
        S: for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut visitor = JsonVisitor::default();
            event.record(&mut visitor);
            let mut log_entry = visitor.0;

            // Capture metadata
            let meta = event.metadata();
            log_entry.insert("level".to_string(), json!(meta.level().to_string()));
            log_entry.insert("target".to_string(), json!(meta.target().to_string()));
            log_entry.insert("name".to_string(), json!(meta.name()));

            // Capture Trace Context if inside a span
            if let Some(span_ref) = _ctx.lookup_current() {
                log_entry.insert("span_name".to_string(), json!(span_ref.name()));
            }

            self.logs.lock().unwrap().push(Value::Object(log_entry));
        }
    }

    #[derive(Default)]
    struct JsonVisitor(serde_json::Map<String, serde_json::Value>);

    impl tracing::field::Visit for JsonVisitor {
        fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
            self.0.insert(field.name().to_string(), json!(value));
        }
        fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
            self.0.insert(field.name().to_string(), json!(value));
        }
        fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
            self.0.insert(field.name().to_string(), json!(value));
        }
        fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
            self.0.insert(field.name().to_string(), json!(value));
        }
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.insert(field.name().to_string(), json!(value));
        }
        fn record_error(
            &mut self,
            field: &tracing::field::Field,
            value: &(dyn std::error::Error + 'static),
        ) {
            self.0
                .insert(field.name().to_string(), json!(value.to_string()));
        }
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0
                .insert(field.name().to_string(), json!(format!("{:?}", value)));
        }
    }

    // --- Telemetry Harness ---

    struct TelemetryHarness {
        metric_exporter: InMemoryMetricExporter,
        meter_provider: SdkMeterProvider,
        span_exporter: InMemorySpanExporter,
        tracer_provider: SdkTracerProvider,
        logs: Arc<Mutex<Vec<serde_json::Value>>>,
        _subscriber_guard: tracing::subscriber::DefaultGuard,
    }

    impl TelemetryHarness {
        fn install() -> Self {
            global::set_text_map_propagator(TraceContextPropagator::new());

            let metric_exporter = InMemoryMetricExporter::default();
            let reader = PeriodicReader::builder(metric_exporter.clone())
                .with_interval(Duration::from_millis(10))
                .build();
            let meter_provider = SdkMeterProvider::builder()
                .with_reader(reader)
                .with_resource(
                    Resource::builder()
                        .with_attributes(vec![KeyValue::new("service.name", "test-app")])
                        .build(),
                )
                .build();
            global::set_meter_provider(meter_provider.clone());

            let span_exporter = InMemorySpanExporter::default();
            let tracer_provider = SdkTracerProvider::builder()
                .with_simple_exporter(span_exporter.clone())
                .with_sampler(Sampler::AlwaysOn)
                .with_resource(
                    Resource::builder()
                        .with_attributes(vec![KeyValue::new("service.name", "test-app")])
                        .build(),
                )
                .build();

            global::set_tracer_provider(tracer_provider.clone());

            let logs = Arc::new(Mutex::new(Vec::new()));
            let log_capture = LogCaptureLayer { logs: logs.clone() };

            let otel_layer =
                tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("test-tracer"));

            let subscriber = Registry::default().with(otel_layer).with(log_capture);

            let guard = tracing::subscriber::set_default(subscriber);

            Self {
                metric_exporter,
                meter_provider,
                span_exporter,
                tracer_provider,
                logs,
                _subscriber_guard: guard,
            }
        }

        fn get_spans(&self) -> Vec<trace::SpanData> {
            self.span_exporter.get_finished_spans().unwrap_or_default()
        }

        fn get_logs(&self) -> Vec<serde_json::Value> {
            self.logs.lock().unwrap().clone()
        }

        fn metric_counts(&self, metric_name: &str, attr_key: Option<&str>) -> HashMap<String, u64> {
            let _ = self.meter_provider.force_flush();
            let metrics = self
                .metric_exporter
                .get_finished_metrics()
                .unwrap_or_default();

            let mut counts = HashMap::new();
            for rm in metrics {
                for scope in rm.scope_metrics() {
                    for metric in scope.metrics() {
                        if metric.name() == metric_name {
                            if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                                for data_point in sum.data_points() {
                                    let label = attr_key
                                        .and_then(|key| {
                                            data_point.attributes().find_map(|kv| {
                                                if kv.key.as_str() == key {
                                                    Some(kv.value.to_string())
                                                } else {
                                                    None
                                                }
                                            })
                                        })
                                        .unwrap_or_else(|| "total".to_string());
                                    *counts.entry(label).or_insert(0) += data_point.value();
                                }
                            }
                        }
                    }
                }
            }
            counts
        }
    }

    impl Drop for TelemetryHarness {
        fn drop(&mut self) {
            global::set_meter_provider(SdkMeterProvider::builder().build());
            global::set_tracer_provider(SdkTracerProvider::builder().build());
        }
    }

    async fn setup_observability_test_app(pool: PgPool) -> axum::Router {
        dotenv().ok();
        let producer: FutureProducer = bookapp::book_ingestion::create_producer().unwrap();
        let db_pools = DatabasePools {
            write_pool: Arc::new(pool.clone()),
            read_pool: Arc::new(pool),
        };

        bookapp::rest::api_router()
            .layer(Extension(
                Arc::new(StubBookDetailsProvider) as Arc<dyn BookDetailsProvider>
            ))
            .layer(Extension(db_pools))
            .layer(Extension(producer))
            .layer(OtelInResponseLayer)
            .layer(OtelAxumLayer::default())
            .layer(
                tower_otel_http_metrics::HTTPMetricsLayerBuilder::builder()
                    .with_meter(opentelemetry::global::meter(env!("CARGO_CRATE_NAME")))
                    .build()
                    .expect("Failed to build otel metrics layer"),
            )
    }

    #[sqlx::test(
        migrations = "../bookapp-dal/migrations",
        fixtures("fixtures/observability_books.sql")
    )]
    async fn test_metrics_capture(pool: PgPool) {
        let harness = TelemetryHarness::install();
        let app = setup_observability_test_app(pool).await;

        let req = Request::builder()
            .uri("/books/search?q=Metrics&limit=10")
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        tokio::time::sleep(Duration::from_millis(20)).await;

        let status_counts = harness.metric_counts("book_search_requests_total", Some("status"));
        println!("Captured Metrics: {:?}", status_counts);
        assert!(
            status_counts.get("success").copied().unwrap_or(0) >= 1,
            "Should record a successful search metric"
        );
    }

    #[sqlx::test(
        migrations = "../bookapp-dal/migrations",
        fixtures("fixtures/observability_books.sql")
    )]
    async fn test_tracing_capture(pool: PgPool) {
        let harness = TelemetryHarness::install();
        let app = setup_observability_test_app(pool).await;

        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=Tracing&limit=10")
            .body(Body::empty())
            .unwrap();

        let _ = app.oneshot(req).await.unwrap();

        let _ = harness.tracer_provider.force_flush();
        let spans = harness.get_spans();

        println!("Captured {} spans", spans.len());
        for span in &spans {
            println!("Span: {} - Attributes: {:?}", span.name, span.attributes);
        }

        assert!(!spans.is_empty(), "Should capture spans");

        let root_span = spans
            .iter()
            .find(|s| s.name == "GET /books/search")
            .expect("Root HTTP span not found");

        let attrs: HashMap<String, String> = root_span
            .attributes
            .iter()
            .map(|kv| (kv.key.as_str().to_string(), kv.value.to_string()))
            .collect();

        let method = attrs
            .get("http.method")
            .or_else(|| attrs.get("http.request.method"));
        let route = attrs.get("http.route");

        assert_eq!(method.map(|s| s.as_str()), Some("GET"));
        assert_eq!(route.map(|s| s.as_str()), Some("/books/search"));

        let service_span = spans
            .iter()
            .find(|s| s.name == "search_books")
            .expect("Should find 'search_books' service span");

        assert_eq!(
            service_span.parent_span_id,
            root_span.span_context.span_id(),
            "Service span should be child of HTTP span"
        );

        let db_span = spans
            .iter()
            .find(|s| s.name == "full_text_search_books_in_db")
            .expect("Should find 'full_text_search_books_in_db' DB span");

        assert_eq!(
            db_span.parent_span_id,
            service_span.span_context.span_id(),
            "DB span should be child of Service span"
        );

        let child_attrs: HashMap<String, String> = db_span
            .attributes
            .iter()
            .map(|kv| (kv.key.as_str().to_string(), kv.value.to_string()))
            .collect();

        assert_eq!(
            child_attrs.get("search.query").map(|s| s.as_str()),
            Some("Tracing")
        );
    }

    #[sqlx::test(
        migrations = "../bookapp-dal/migrations",
        fixtures("fixtures/observability_books.sql")
    )]
    async fn test_logging_capture(pool: PgPool) {
        let harness = TelemetryHarness::install();
        let app = setup_observability_test_app(pool).await;

        let req = Request::builder()
            .uri("/books/search?q=&limit=10")
            .body(Body::empty())
            .unwrap();

        let _ = app.oneshot(req).await.unwrap();

        let logs = harness.get_logs();
        assert!(!logs.is_empty(), "Should capture logs");

        let warning_log = logs.iter().find(|l| {
            l["level"].as_str() == Some("WARN") && l["target"].as_str() == Some("bookapp::rest")
        });

        assert!(
            warning_log.is_some(),
            "Should find a WARN log from bookapp::rest"
        );
    }

    #[sqlx::test(
        migrations = "../bookapp-dal/migrations",
        fixtures("fixtures/observability_books.sql")
    )]
    async fn test_e2e_correlation(pool: PgPool) {
        let harness = TelemetryHarness::install();
        let app = setup_observability_test_app(pool).await;

        let req = Request::builder()
            .method("GET")
            .uri("/books/search?q=Correlation&limit=10")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        let headers = response.headers();
        let traceparent = headers
            .get("traceparent")
            .or_else(|| headers.get("tracestate"))
            .expect("Response should have traceparent header")
            .to_str()
            .unwrap();

        let parts: Vec<&str> = traceparent.split('-').collect();
        assert_eq!(parts.len(), 4, "Invalid traceparent format");
        let response_trace_id = parts[1];

        println!("Client received Trace ID: {}", response_trace_id);

        let _ = harness.tracer_provider.force_flush();
        let spans = harness.get_spans();

        let span_trace_ids: Vec<String> = spans
            .iter()
            .map(|s| format!("{:032x}", s.span_context.trace_id()))
            .collect();

        assert!(
            span_trace_ids.iter().any(|tid| tid == response_trace_id),
            "Captured spans should contain the Trace ID returned to the client"
        );

        let logs = harness.get_logs();
        let info_log = logs.iter().find(|l| {
            l.get("message").and_then(|m| m.as_str())
                == Some("Full-text search completed successfully")
        });

        if let Some(log) = info_log {
            assert_eq!(log["span_name"], "full_text_search_books_in_db");
        }
    }
}
