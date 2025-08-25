# tokio-otel-metrics

[![Crates.io](https://img.shields.io/crates/v/tokio-otel-metrics.svg)](https://crates.io/crates/tokio-otel-metrics)
[![Documentation](https://docs.rs/tokio-otel-metrics/badge.svg)](https://docs.rs/tokio-otel-metrics)
[![License: Apache-2.0](https://img.shields.io/crates/l/tokio-otel-metrics.svg)](#license)

OpenTelemetry metrics collection for Tokio runtime, compatible with OpenTelemetry 0.30+.

This crate provides metrics collection for Tokio applications, offering visibility into runtime performance, task scheduling 
behavior, and resource utilization through OpenTelemetry's efficient callback-based metric system.

## Features

- 🚀 **Low-overhead collection** using OpenTelemetry observable instruments
- 📊 **Comprehensive metrics** covering runtime, workers, and tasks
- 🔧 **OpenTelemetry 0.30+ compatible** for modern observability stacks
- 💾 **Optional memory metrics** via `memory-stats` crate

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
tokio-otel-metrics = "0.1.0"
opentelemetry_sdk = "0.30.0"
```

Basic usage:

```rust
use opentelemetry_sdk::metrics::SdkMeterProvider;
use tokio_otel_metrics::TokioRuntimeMetrics;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize OpenTelemetry
    let meter_provider = SdkMeterProvider::default();
    let meter = meter_provider.meter("tokio_runtime");
    
    // Register Tokio runtime metrics
    let _registrations = TokioRuntimeMetrics::register(&meter)?;
    
    // Your application code here...
    // Metrics are collected automatically in the background
    
    Ok(())
}
```

## Available Metrics

### Runtime-Level Metrics

| Metric Name | Type | Description |
|-------------|------|-------------|
| `tokio_workers_count` | Gauge | Number of worker threads in the runtime |
| `tokio_alive_tasks` | Gauge | Number of currently alive tasks |
| `tokio_global_queue_depth` | Gauge | Depth of the global task queue |
| `tokio_blocking_threads` | Gauge | Number of blocking threads |
| `tokio_spawned_tasks_total` | Gauge | Total tasks spawned since runtime creation* |
| `tokio_blocking_queue_depth` | Gauge | Depth of the blocking task queue* |

### Per-Worker Metrics

| Metric Name | Type | Description | Labels |
|-------------|------|-------------|--------|
| `tokio_worker_busy_duration_seconds` | Gauge | Time each worker has been busy | `worker_id` |
| `tokio_worker_park_count` | Gauge | Times each worker has parked | `worker_id` |
| `tokio_worker_poll_count` | Gauge | Tasks polled by each worker | `worker_id` |

### Memory Metrics (Optional)

Enable with the `memory-metrics` feature:

```toml
tokio-otel-metrics = { version = "0.1.0", features = ["memory-metrics"] }
```

| Metric Name | Type | Description |
|-------------|------|-------------|
| `process_memory_usage_bytes` | Gauge | Process memory usage (RSS) |

*\* Requires `target_has_atomic="64"` on the target platform*

## Advanced Usage

### Registering All Metrics

```rust
use tokio_otel_metrics::register_all_metrics;

let _registrations = register_all_metrics(&meter)?;
```

### Memory Metrics

```rust
#[cfg(feature = "memory-metrics")]
use tokio_otel_metrics::ProcessMemoryMetrics;

#[cfg(feature = "memory-metrics")]
let _memory_registration = ProcessMemoryMetrics::register(&meter)?;
```

### Using with OpenTelemetry Exporters

```rust
use opentelemetry_otlp::MetricsExporterBuilder;
use opentelemetry_sdk::metrics::SdkMeterProvider;

// Configure OTLP exporter
let exporter = opentelemetry_otlp::MetricExporter::builder()
    .with_tonic()
    .build()?;

let meter_provider = SdkMeterProvider::builder()
    .with_periodic_exporter(exporter)
    .build();

// Register metrics
let meter = meter_provider.meter("tokio_runtime");
let _registrations = tokio_otel_metrics::register_all_metrics(&meter)?;
```

## Compilation Flags

For maximum metric coverage, especially on unstable Tokio features:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo build
```

This enables additional runtime metrics that may not be available in stable Tokio builds.

Memory metrics are provided by the [`memory-stats`](https://crates.io/crates/memory-stats) crate and support all major platforms.

## Comparison with Other Crates

| Feature | tokio-otel-metrics | runtime-otel-rs |
|---------|-------------------|-----------------|
| OpenTelemetry 0.30+ | ✅ | ❌ |
| Zero-overhead collection | ✅ | ✅ |
| Worker-specific metrics | ✅ | ✅ |
| Memory metrics | ✅ (optional) | ✅ |
| Production ready | ✅ | Experimental |
| Comprehensive documentation | ✅ | Limited |

## Examples

See the [`examples/`](examples/) directory for complete working examples:

- [`basic.rs`](examples/basic.rs) - Simple runtime metrics collection
- [`with_memory.rs`](examples/with_memory.rs) - Runtime + memory metrics
- [`prometheus_export.rs`](examples/prometheus_export.rs) - Export to Prometheus
- [`otlp_export.rs`](examples/otlp_export.rs) - Export via OTLP

## Testing

Run the test suite:

```bash
# Basic tests
cargo test

# Tests with memory metrics
cargo test --features memory-metrics

# Tests with unstable features
RUSTFLAGS="--cfg tokio_unstable" cargo test --features memory-metrics
```

## Contributing

Contributions are welcome! Please read our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
cd tokio-otel-metrics
cargo build
cargo test
```

## License

Licensed under the Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed under the Apache License, Version 2.0, without any additional terms or conditions.