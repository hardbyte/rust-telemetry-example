# Agent Reference Guide

## Quick Commands

### Build, Lint, Test
```bash
# Start services (Compose v2.20+ for --wait)
docker compose up -d --wait --wait-timeout 180          # Full stack
docker compose up -d --wait --wait-timeout 120 db       # Database only

# Set DATABASE_URL for local development
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"

# Format and lint (always run before commit)
cargo fmt --all
SQLX_OFFLINE=true cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build and test
SQLX_OFFLINE=true cargo build --workspace
SQLX_OFFLINE=true cargo test --workspace

# Specific tests
cargo test -p backend test_rate_limited          # Unit test
cargo test -p tests test_alert_detection_capability  # Integration test (requires stack)

# SQLx workflow (after DB schema changes)
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
export ERROR_INJECTION_DATABASE_URL="${DATABASE_URL}?options=--search_path%3Derror_injection"

cd bookapp-dal
sqlx migrate run
cargo sqlx prepare
cd ../error-injection-dal
DATABASE_URL="$ERROR_INJECTION_DATABASE_URL" sqlx migrate run
DATABASE_URL="$ERROR_INJECTION_DATABASE_URL" cargo sqlx prepare
cd ..
git add bookapp-dal/.sqlx error-injection-dal/.sqlx && git commit -m "Update SQLx metadata"
```

### Load Testing
```bash
# Basic load test
uvx locust -f requests/locustfile.py

# With OTEL integration
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
uvx --with 'opentelemetry-sdk' \
  --with "opentelemetry-exporter-otlp-proto-grpc >=1.24.0" \
  --with "opentelemetry-instrumentation-requests==0.46b0" \
  locust -f requests/locustfile.py --headless -u 50 -r 5 -t 1m
```

### Service Endpoints

When running `docker compose up`, services use **dynamic port mapping**:

```bash
# Discover mapped ports
APP_PORT=$(docker compose port app 8000 | cut -d: -f2)
GRAFANA_PORT=$(docker compose port telemetry 3000 | cut -d: -f2)
OTLP_GRPC=$(docker compose port telemetry 4317 | cut -d: -f2)

echo "API:     http://localhost:${APP_PORT}"
echo "Grafana: http://localhost:${GRAFANA_PORT}"
echo "OTLP:    http://localhost:${OTLP_GRPC}"
```

**Default development ports** (when services expose ports directly):
- API: `http://localhost:8000`
- Grafana: `http://localhost:3000` (admin:admin)
- OTLP gRPC: `http://localhost:4317`
- OTLP HTTP: `http://localhost:4318`
- Tempo: `http://localhost:3200`
- Tokio Console: `localhost:6669` (bookapp), `localhost:6670` (backend)

## Code Style Guidelines

### Formatting & Imports
- Use rustfmt defaults (enforced by CI)
- Group imports: `std` → third-party → crate-local
- Avoid glob imports (`use foo::*`)
- No unused imports or dead code

### Types & Naming
- Explicit return types on public functions
- Descriptive names (no single-letter variables except iterators)
- `snake_case` for functions/variables
- `CamelCase` for types
- `SCREAMING_SNAKE_CASE` for constants

### Error Handling
- Binaries: `anyhow::Result` with `.context()` for rich errors
- Libraries: `thiserror` for custom error types
- Avoid `unwrap()`/`expect()` - use `?` operator
- Add context at each layer: `.context("failed to connect to database")?`

### Logging & Observability
- Use `tracing` crate (never `println!`)
- Structured fields: `tracing::info!(user_id = %id, "User created")`
- `#[tracing::instrument]` for async functions and HTTP handlers
- Consider span creation for critical paths

### Concurrency & Async
- Tokio runtime exclusively
- Keep `.cargo/config.toml` with `rustflags = ["--cfg", "tokio_unstable"]`
- Use `tokio::spawn` for concurrent tasks
- Avoid blocking operations in async context

### Database (SQLx)
- Compile-time query verification via `bookapp-dal/.sqlx/` and `error-injection-dal/.sqlx/` metadata
- After query changes: run `cargo sqlx prepare` inside the crate you touched (remember to set `ERROR_INJECTION_DATABASE_URL` when preparing the error-injection DAL)
- Always commit both directories when they change
- Use repository pattern (see `bookapp-dal` and `error-injection-dal`)

### General Principles
- Keep builds warning-free (`-D warnings`)
- Small, focused PRs with minimal diffs
- Idiomatic Rust - follow ecosystem conventions
- Document public APIs with `///` doc comments

## Crate Structure

```
rust-telemetry-example/
├── bookapp/              # REST API service (port 8000)
├── backend/              # Kafka consumer + scheduler (console: 6670)
├── bookapp-dal/          # Data access layer with SQLx
├── error-injection-dal/  # Error injection config store + schema
├── client/               # Progenitor-generated API client
├── data-loader/          # Bulk data loader utility
├── tests/                # Integration tests (telemetry, alerts)
├── tokio-otel-metrics/   # Custom Tokio runtime metrics
├── observability-utils/  # Shared tracing/metrics config
└── telemetry-config/     # Grafana dashboards & OTEL config
```

## Testing

### Unit Tests
```bash
cargo test -p bookapp              # API service tests
cargo test -p backend              # Backend worker tests
cargo test -p bookapp-dal          # DAL tests (requires DB)
```

### Integration Tests
```bash
# Requires full stack running
docker compose up -d

# Run all integration tests (full stack required)
cargo test -p tests -- --nocapture

# Specific test suites
cargo test -p tests --test telemetry_test           # Telemetry validation
cargo test -p tests --test alert_framework_test     # Alert framework
cargo test -p tests --test alert_validation_test    # Alert validation
cargo test -p tests --test failure_tests            # Failure scenarios
```

### Test Script
```bash
SQLX_OFFLINE=true ./run_tests.sh    # Formats, lints, builds, tests entire workspace
```

### Soak / Flake testing
```bash
INTEGRATION_LOOP_ITERATIONS=30 ./scripts/integration_loop.sh
# adjust with INTEGRATION_LOOP_TIMEOUT_SECS=1200 for longer test timeout
```

## Key Files

- **CLAUDE.md** - Project-specific instructions for AI assistants
- **README.md** - Comprehensive project documentation
- **docker-compose.yaml** - Service definitions and configuration
- **.cargo/config.toml** - Tokio unstable flags for console
- **telemetry-config/** - Observability configuration and dashboards
- **run_tests.sh** - Automated test runner script (Compose `--wait` aware)
- **scripts/integration_loop.sh** - Long-running soak harness for integration tests

## Common Workflows

### Adding a New Database Query
1. Modify query in the appropriate DAL crate (`bookapp-dal/` for catalog data, `error-injection-dal/` for latency/error config)
2. Run `sqlx migrate run` / `cargo sqlx prepare` inside that crate (remember to export `ERROR_INJECTION_DATABASE_URL=...` before touching the error-injection DAL)
3. Commit the updated metadata: `git add bookapp-dal/.sqlx` or `git add error-injection-dal/.sqlx` (or both)

### Adding a New Endpoint
1. Define in OpenAPI spec or `bookapp/src/rest.rs`
2. Add handler with `#[tracing::instrument]`
3. Run `cargo build` to regenerate client (if using Progenitor)
4. Add integration test in `tests/`

### Debugging Observability
1. Check Grafana: `http://localhost:3000`
2. View traces in Tempo, logs in Loki, metrics in Prometheus
3. Use Sentry correlation: Copy `otel.trace_id` → Search in Grafana
4. Tokio console: `tokio-console http://localhost:6669` (requires `tokio-console` CLI)

### Performance Testing
1. Start stack: `docker compose up -d`
2. Run locust: `uvx locust -f requests/locustfile.py`
3. Monitor in Grafana dashboard: "BookApp Service Overview"
4. Check Tokio metrics: "Tokio Runtime Observability Dashboard"

## Dependencies

### Core Dependencies
- **axum** - Web framework
- **sqlx** - Async PostgreSQL client with compile-time verification
- **rdkafka** - Kafka client
- **tokio** - Async runtime
- **tracing** - Structured logging and tracing
- **opentelemetry** (0.30+) - Telemetry SDK
- **sentry** - Error tracking with OTEL correlation

### Development Tools
- **cargo-sqlx** - SQLx CLI for migrations and query prep
- **locust** - Load testing (via uvx)
- **tokio-console** - Tokio runtime debugger
- **progenitor** - OpenAPI client generator

## Environment Variables

```bash
# Database
DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
ERROR_INJECTION_DATABASE_URL="${DATABASE_URL}?options=--search_path%3Derror_injection"

# OpenTelemetry
OTEL_SERVICE_NAME="bookapp"
OTEL_EXPORTER_OTLP_ENDPOINT="http://localhost:4317"

# Sentry (optional)
SENTRY_DSN="https://your-key@sentry.io/your-project-id"
SENTRY_ENVIRONMENT="development"
SENTRY_RELEASE="bookapp@1.0.0"

# Kafka
KAFKA_BROKER_URL="kafka:9092"
ENABLE_KAFKA_PRODUCER="true"

# Search refresh (backend)
SEARCH_REFRESH_MIN_INTERVAL_SECS="30"
SEARCH_REFRESH_MAX_STALENESS_SECS="300"
```

See `.env.example` for complete configuration.
