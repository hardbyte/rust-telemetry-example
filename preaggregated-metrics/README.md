# Exporting preaggregated histograms with OpenTelemetry

This example pushes existing histogram bucket populations directly over OTLP/gRPC,
using the generated Rust messages and client in `opentelemetry-proto`. It does not
call a histogram instrument's `record()` method and does not expose a scrape endpoint.

This is useful when a database or another system owns the aggregated data. The
ordinary OpenTelemetry Histogram instrument accepts individual observations; the
OTLP data model also represents already-aggregated distributions. This example uses
that data model directly, bypassing the SDK's aggregation pipeline.

## Run the demo

From the workspace root:

```sh
docker compose --profile default up -d --build
```

Open [Preaggregated OTLP Histograms](http://localhost:3000/d/preaggregated-histograms)
in Grafana. Allow about one minute for rate queries to collect enough samples.
The `histogram-demo` service supplies synthetic cumulative snapshots every five
seconds. Each update adds 11 observations with total duration 5.6 seconds. Expect
about 2.2 observations/second and a mean of 0.509 seconds. The p95 is an interpolated
estimate, about 1.675 seconds for these bucket populations.

To run the producer on the host against an existing collector:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
  cargo run -p preaggregated-metrics -- --demo
```

The producer has no Prometheus dependency. The existing demo stack forwards OTLP
to its metrics backend for Grafana queries; any OTLP-compatible receiver can
replace that destination.

## Export your own snapshot

Save a JSON snapshot from your aggregation source:

```json
{
  "start_time_unix_nano": 1700000000000000000,
  "time_unix_nano": 1700000060000000000,
  "explicit_bounds": [0.1, 0.5, 1.0, 2.5],
  "bucket_counts": [2, 5, 3, 1, 0],
  "sum": 5.6
}
```

```sh
cargo run -p preaggregated-metrics -- --snapshot snapshot.json
```

These illustrative timestamps are historical. Supply actual aggregation start and
snapshot timestamps when sending live data to a backend with retention limits.
Durations and bounds are in seconds; timestamps are Unix nanoseconds.

`bucket_counts` contains **disjoint populations**, not running totals across bounds:
2 observations at or below 0.1 s, 5 in (0.1, 0.5], 3 in (0.5, 1.0], 1 in
(1.0, 2.5], and none above 2.5 s. The final bucket is implicit overflow, so there
is always one more bucket than explicit bounds. The example derives `count` by
summing these populations and preserves the supplied sum. It does not invent min,
max or exemplars from bucket boundaries.

## Cumulative time and restarts

The histogram is cumulative **over time**: each snapshot describes all observations
since `start_time_unix_nano`. This is independent of whether its buckets overlap;
OTLP buckets never overlap across boundaries.

If the database retains the cumulative population, retain its aggregation start
timestamp too. After restarting the exporter, send the same start and the updated
snapshot end time. Do not reset the start to the exporter's process start while
continuing to send historical counts. Keep bounds and resource identity stable,
and have only one exporter own a series. Database retention or rebuilding the
population can require a new aggregation epoch.

The synthetic `--demo` process starts a new population when restarted and therefore
uses a new start timestamp. `--snapshot` preserves both timestamps from the file.
Neither mode replays old observations through a long-lived SDK histogram, which
would count them repeatedly. Sending a historical population does not backfill
individual historical metric samples into a monitoring backend.

## Scope and validation

This is an example sender, not a general-purpose exporter. It checks bucket shape,
bounds, count overflow and timestamps, applies connection/request timeouts, and
reports partial rejection. It exits on export failure and does not implement a
persistent retry queue, authentication configuration or automatic scheduling for
database snapshots. The demonstration endpoint is a local plaintext gRPC listener.

```sh
cargo test -p preaggregated-metrics
```

Tests send requests to a local gRPC receiver and verify the exact histogram,
cumulative start across new connections, and rejection handling.

References: [OTel Histogram instrument](https://opentelemetry.io/docs/specs/otel/metrics/api/#histogram),
[OTLP metric data model](https://opentelemetry.io/docs/specs/otel/metrics/data-model/#histogram),
[generated Rust histogram message](https://docs.rs/opentelemetry-proto/0.32.0/opentelemetry_proto/tonic/metrics/v1/struct.HistogramDataPoint.html).
