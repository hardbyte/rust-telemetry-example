use tokio_otel_metrics::TokioRuntimeMetrics;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_otlp::MetricExporter;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🚀 Starting tokio-otel-metrics validation test...");
    
    // Configure OTLP exporter to send to local collector
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint("http://localhost:4317")
        .build()?;
    
    // Configure resource attributes
    let resource = Resource::new([
        resource::SERVICE_NAME.string("tokio-metrics-test"),
        resource::SERVICE_VERSION.string("1.0.0"),
    ]);
    
    // Create meter provider with export configuration
    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(exporter, Duration::from_secs(5))
        .with_resource(resource)
        .build();
    
    // Get meter for our service
    let meter = meter_provider.meter("tokio-metrics-test");
    
    println!("📊 Registering Tokio runtime metrics...");
    
    // Register all Tokio runtime metrics
    let _registrations = TokioRuntimeMetrics::register(&meter)?;
    
    println!("✅ Successfully registered Tokio runtime metrics");
    
    // Create some async workload to generate metrics
    println!("🔄 Generating workload to create metrics data...");
    
    let mut tasks = Vec::new();
    
    // Spawn multiple tasks to generate runtime activity
    for i in 0..10 {
        let task = tokio::spawn(async move {
            for j in 0..100 {
                // Simulate some work
                tokio::time::sleep(Duration::from_millis(10)).await;
                
                // Some CPU work
                let mut sum = 0;
                for k in 0..1000 {
                    sum += i * j * k;
                }
                
                if j % 50 == 0 {
                    println!("Task {} completed iteration {}", i, j);
                }
            }
        });
        tasks.push(task);
    }
    
    // Wait for some tasks to complete while metrics are being collected
    for (idx, task) in tasks.into_iter().enumerate() {
        task.await?;
        println!("✅ Task {} completed", idx);
    }
    
    println!("🔄 Keeping metrics running for 30 seconds to allow collection...");
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    // Force a final metric export
    meter_provider.force_flush()?;
    
    println!("✅ Tokio metrics test completed successfully!");
    println!("📈 Check Grafana at http://localhost:3000 for the Tokio Runtime Observability dashboard");
    
    Ok(())
}