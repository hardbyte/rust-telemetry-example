// In tests/src/lib.rs
pub mod telemetry_test;

use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::sync::Once;
use opentelemetry_sdk::trace::{TracerProvider as SdkTracerProvider, BatchSpanProcessor}; // Added BatchSpanProcessor
use opentelemetry_otlp::WithExportConfig; // Required for endpoint configuration

static INIT_TRACING: Once = Once::new();
static mut GLOBAL_TRACER_PROVIDER: Option<SdkTracerProvider> = None;

// Minimal OpenTelemetry setup for the test client
pub fn init_test_tracing_provider() {
    INIT_TRACING.call_once(|| {
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
            .with_batch_exporter(exporter) // Changed to with_batch_exporter
            .with_config(opentelemetry_sdk::trace::Config::default().with_resource(
                opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    "integration-tester", // Unique service name for the test runner
                )]),
            ))
            .build();
        
        opentelemetry::global::set_tracer_provider(provider.clone());
        unsafe {
            GLOBAL_TRACER_PROVIDER = Some(provider);
        }
    });
}

pub fn flush_traces() {
    unsafe {
        if let Some(provider) = &GLOBAL_TRACER_PROVIDER {
            provider.force_flush();
             println!("Global tracer provider found, calling force_flush.");
        } else {
            println!("Global tracer provider not found during flush_traces.");
        }
    }
     // Ensure a short period for flushing to complete, even after force_flush.
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!("Finished flush_traces.");
}
