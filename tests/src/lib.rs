// In tests/src/lib.rs
pub mod telemetry_test;

use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::sync::OnceLock; // Required for endpoint configuration

static GLOBAL_TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

// Minimal OpenTelemetry setup for the test client
pub fn init_test_tracing_provider() {
    GLOBAL_TRACER_PROVIDER.get_or_init(|| {
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://telemetry:4317".to_string());
        println!("Test OTLP Exporter Endpoint: {}", otlp_endpoint);

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic() // Using tonic gRPC exporter
            .with_endpoint(otlp_endpoint.clone()) // Target the telemetry service from docker-compose
            .build()
            .expect("Failed to create OTLP span exporter for tests");

        let provider = SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .build();

        opentelemetry::global::set_tracer_provider(provider.clone());
        provider
    });
}

pub fn flush_traces() {
    if let Some(provider) = GLOBAL_TRACER_PROVIDER.get() {
        let _ = provider.force_flush();
        println!("Global tracer provider found, calling force_flush.");
    } else {
        println!("Global tracer provider not found during flush_traces.");
    }
    // Ensure a short period for flushing to complete, even after force_flush.
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!("Finished flush_traces.");
}
