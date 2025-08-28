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
- 💾 **Optional memory metrics** via `memory-metrics` feature
- 👷 **Optional per-worker metrics** via `worker-metrics` feature
- 🏗️ **RAII resource management** with automatic metric cleanup

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

### Runtime-Level Metrics (Always Available)

| Metric Name | Type | Unit | Description |
|-------------|------|------|-------------|
| `tokio.runtime.workers` | Gauge | `1` | Number of worker threads in the runtime |
| `tokio.runtime.tasks.active` | Gauge | `1` | Number of currently active tasks |
| `tokio.runtime.queue.depth` | Gauge | `1` | Depth of the global task queue |
| `tokio.runtime.threads.blocking` | Gauge | `1` | Number of blocking threads |
| `tokio.runtime.tasks.spawned` | Counter | `1` | Total tasks spawned since runtime creation* |
| `tokio.runtime.blocking.queue.depth` | Gauge | `1` | Depth of the blocking task queue* |

*Requires `target_has_atomic="64"` on the target platform.

### Per-Worker Metrics (`worker-metrics` feature)

Enable with the `worker-metrics` feature:

```toml
tokio-otel-metrics = { version = "0.1.0", features = ["worker-metrics"] }
```

| Metric Name | Type | Unit | Description | Labels |
|-------------|------|------|-------------|--------|
| `tokio.runtime.worker.busy_time` | Counter | `s` | Total time each worker has been busy | `worker.id` |
| `tokio.runtime.worker.parks` | Counter | `1` | Number of times each worker has parked | `worker.id` |
| `tokio.runtime.worker.polls` | Counter | `1` | Number of tasks polled by each worker | `worker.id` |
| `tokio.runtime.worker.steals` | Counter | `1` | Number of tasks stolen by each worker | `worker.id` |
| `tokio.runtime.worker.overflows` | Counter | `1` | Number of overflow events by each worker | `worker.id` |
| `tokio.runtime.worker.queue.depth` | Gauge | `1` | Local queue depth for each worker | `worker.id` |

### Task-Level Metrics

| Metric Name | Type | Unit | Description |
|-------------|------|------|-------------|
| `tokio.tasks.completed` | Counter | `1` | Number of manually tracked completed tasks |
| `tokio.runtime.tasks.slow_start` | Counter | `1` | Tasks with >100ms spawn-to-poll delay |
| `tokio.runtime.polls.total` | Counter | `1` | Total number of task polls across all tasks |
| `tokio.runtime.poll.duration.total` | Counter | `s` | Total time spent in task polls |
| `tokio.runtime.poll.duration` | Histogram | `s` | Distribution of individual task poll durations |

### Memory Metrics (`memory-metrics` feature)

Enable with the `memory-metrics` feature:

```toml
tokio-otel-metrics = { version = "0.1.0", features = ["memory-metrics"] }
```

| Metric Name | Type | Unit | Description | Labels |
|-------------|------|------|-------------|--------|
| `process.memory.usage` | Gauge | `By` | Process memory usage | `state="rss"` |

## Advanced Usage

### Registering All Metrics

```rust
use tokio_otel_metrics::register_all_metrics;

let _registrations = register_all_metrics(&meter)?;
```

### Task-Level Metrics

For detailed task tracking, use the `TaskMetrics` collector:

```rust
use tokio_otel_metrics::TaskMetrics;

let task_metrics = TaskMetrics::new();
let _registrations = task_metrics.register_metrics(&meter)?;

// Track individual tasks
let task_id = tokio::spawn(async { /* work */ }).id();
task_metrics.task_spawned(task_id, "my_task");
task_metrics.task_polled(task_id, Duration::from_millis(5));
task_metrics.task_completed(task_id);
```

### Memory Metrics

```rust
#[cfg(feature = "memory-metrics")]
use tokio_otel_metrics::ProcessMemoryMetrics;

#[cfg(feature = "memory-metrics")]
let _memory_registrations = ProcessMemoryMetrics::register(&meter)?;
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

| Feature                     | tokio-otel-metrics | runtime-otel-rs |
|-----------------------------|-------------------|-----------------|
| OpenTelemetry 0.30+         | ✅ | ❌ |
| Low-overhead collection     | ✅ | ✅ |
| Worker-specific metrics     | ✅ | ✅ |
| Memory metrics              | ✅ (optional) | ✅ |


## Testing

Run the test suite:

```bash
# Basic tests
cargo test

# Tests with memory metrics
cargo test --features memory-metrics

# Tests with worker metrics
cargo test --features worker-metrics

# Tests with all features
cargo test --all-features

# Tests with unstable features
RUSTFLAGS="--cfg tokio_unstable" cargo test --all-features
```


## Development Setup

```bash
cd tokio-otel-metrics
cargo build
cargo test
```

## License

Licensed under the Apache License, Version 2.0 http://www.apache.org/licenses/LICENSE-2.0.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed under the Apache License, Version 2.0, without any additional terms or conditions.