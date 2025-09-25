# Rust Telemetry & Observability Example

This repository provides a comprehensive example of cross-service telemetry and observability in a distributed Rust application. It demonstrates how to effectively implement the three pillars of observability—traces, metrics, and logs—using OpenTelemetry and the Rust ecosystem.

The project is a multi-service "Book App" designed to showcase how to trace requests as they flow through REST APIs, asynchronous message queues (Kafka), and database interactions.

## Architecture

The system consists of two Rust services, a separate data access layer (DAL), a PostgreSQL database, and a Kafka message queue. All telemetry data is collected by the OpenTelemetry Collector and visualized using Grafana, Loki, Tempo, and Prometheus. Sentry is integrated for advanced error tracking and is correlated with the OpenTelemetry traces.

### Crate Structure

This is a Cargo workspace with the following crates:

- **`bookapp`**: Main REST API service (port 8000) - handles HTTP requests, produces Kafka messages
- **`bookapp-dal`**: Data access layer with repository pattern, SQLx integration, and compile-time query verification
- **`backend`**: Async message processor and background task scheduler - consumes Kafka messages, runs scheduled jobs, includes outbox publisher (console-subscriber on 6670)
- **`data-loader`**: Bulk data loading utility using the Progenitor client - demonstrates cross-service tracing for batch operations
- **`client`**: Generated API client using Progenitor for type-safe service-to-service calls with automatic tracing integration
- **`tests`**: Integration tests for end-to-end telemetry validation, including data-loader testing
- **`tokio-otel-metrics`**: Custom Tokio runtime observability crate with OpenTelemetry 0.30+ observable instruments
- **`observability-utils`**: Shared telemetry utilities for tracing, metrics, Sentry integration, and Tokio metrics initialization


### Service and Data Flow
```mermaid
graph LR
  subgraph User Input
    direction LR
    user[<fa:fa-user> User]
    locust[<fa:fa-bug> Locust]
  end

  subgraph Application
    app[<fa:fa-server> BookApp Service]
    backend[<fa:fa-gears> Backend Worker]
    db[(<fa:fa-database> PostgreSQL)]

    subgraph Async Flow
      direction TB
      kafka[<fa:fa-exchange> Kafka]
    end
  end

  user -- "POST /books/add" --> app
  locust -- "Generates Load" --> app

  app -- "1. Writes to DB" --> db
  app -- "2. Produces Message" --> kafka

  kafka -- "Delivers Message" --> backend
  backend -- "Processes & Reads from DB" --> db
  backend -- "Scheduled Tasks" --> db

  classDef services fill:#f9f,stroke:#333,stroke-width:2px;
  class app,backend,kafka,db services;
```

### Observability Pipeline

```mermaid
graph TD
    subgraph "Data Sources"
        direction LR
        app[<fa:fa-server> BookApp Service]
        backend[<fa:fa-gears> Backend Worker]
        infra["<fa:fa-database> Kafka & PostgreSQL"]
    end

    subgraph "Collection & Processing"
        collector[<fa:fa-filter> OpenTelemetry Collector]
    end

    subgraph "Observability Backend"
        direction LR
        sentry["<fa:fa-bell> Sentry<br/>(Error Tracking)"]
        lgtm["<fa:fa-chart-bar> Grafana Stack<br/>(Tempo, Loki, Prometheus)"]
    end

    app -- "Traces, Metrics, Logs" --> collector
    backend -- "Traces, Metrics, Logs" --> collector
    app -- "Errors with Trace ID" --> sentry
    backend -- "Errors with Trace ID" --> sentry
    collector -- "Scrapes Infra Metrics" --> infra
    collector -- "Processed Telemetry" --> lgtm
    sentry -.->|Link via Trace ID| lgtm

    classDef apps fill:#f9f,stroke:#333,stroke-width:2px;
    class app,backend,infra apps;
    classDef telemetry fill:#9f9,stroke:#333,stroke-width:2px;
    class collector telemetry;
    classDef backendobs fill:#99f,stroke:#333,stroke-width:2px;
    class sentry,lgtm backendobs;
```

## Key Features

This repository demonstrates several production-ready observability patterns:

- **Distributed Tracing**: End-to-end tracing across multiple services and protocols:
    - **HTTP**: The `axum` web framework is instrumented to create and propagate trace context.
    - **Database**: `sqlx` database calls are traced to monitor query performance via the dedicated DAL crate.
    - **Generated Client**: An OpenAPI-generated progenitor client is instrumented to propagate context automatically.
    - **Message Queue (Kafka)**: Trace context is injected into Kafka message headers and used to create linked spans in the consumer, correctly modeling the asynchronous workflow.

- **Sentry & OpenTelemetry Correlation**: A custom `SentryOtelCorrelationLayer` bridges the gap between Sentry error tracking and OpenTelemetry tracing. When an error is sent to Sentry, it is automatically tagged with the `otel.trace_id` and `otel.span_id`. This allows you to jump directly from a Sentry issue to the corresponding distributed trace in Grafana/Tempo for immediate context.
- **Logs with Trace Context**: Application logs are exported via the OpenTelemetry Collector and are automatically correlated with traces, making it easy to find logs for a specific request.
- **Automatic Metrics Generation**: In addition to custom application metrics, the OTEL Collector is configured to generate span metrics and service graphs directly from trace data. This provides RED metrics (Rate, Errors, Duration) with minimal instrumentation effort.
- **Configurable Error Injection**: A middleware is included that can be configured at runtime to inject errors for specific API endpoints. This is a powerful tool for testing system resilience, alerts, and error-tracking integrations.
- **Instrumented Load Testing**: The included Locust load testing script is itself instrumented with OpenTelemetry, allowing you to trace requests originating from the load generator all the way through the system.
- **Health Monitoring**: Dedicated `/health` endpoint for application health checks, monitored by OpenTelemetry Collector's httpcheck receiver without generating traces, keeping observability data clean.


![dashboard.png](.github/dashboard.png)

## A Deeper Look

### Tracing

The **tracing** crate is used to instrument the application code. The `tracing-opentelemetry` crate exports this data to the 
OpenTelemetry Collector. The implementation demonstrates context propagation for:

- **HTTP**: Using `axum-tracing-opentelemetry` for server-side and `reqwest-tracing` for client-side propagation.
- **Generated Client**: An OpenAPI-generated progenitor client is hooked to inject trace headers automatically.
- **Kafka**: Trace context is passed via message headers and used to create linked spans in the consumer.


**Span metrics** are calculated from the trace data using the otel collector along with exemplars.

Visualized natively in Grafana:

![img.png](.github/spanmetrics.png)


### Metrics

The project implements comprehensive metrics collection across multiple layers:

- **Application Metrics**: Custom business metrics via OpenTelemetry meter API
- **HTTP Request Metrics**: Request/response metrics with exemplars via `tower-otel-http-metrics`  
- **Tokio Runtime Metrics**: Comprehensive async runtime observability via custom `tokio-otel-metrics` crate
- **Infrastructure Metrics**: PostgreSQL and Kafka metrics via OpenTelemetry Collector
- **Span Metrics**: Automatically generated RED metrics (Rate, Errors, Duration) from trace data

All metrics are exported to Prometheus and visualized in Grafana with pre-built dashboards.

![img.png](.github/metrics.png)


You can carry out metric queries against traces in Grafana:

```promql
```traceql
{ event.target ="sqlx::query" && event.summary =~ "insert.*"} | quantile_over_time(duration, 0.95) by (event.summary)
```

### Logging

Log events are both shown on the console and exported to the **OpenTelemetry Collector** using
the `OpenTelemetryTracingBridge` - these can be viewed in Loki.

![img.png](.github/loki.png)


### Error Tracking & Correlation

Sentry is integrated for robust error tracking. The custom SentryOtelCorrelationLayer bridges the two ecosystems, providing a unified debugging experience.
When an error occurs:

**Key Features:**
- **Structured Log Capture**: Sentry's `logs` feature captures structured log events with rich context
- **Automatic Trace Correlation**: OpenTelemetry trace IDs are automatically embedded as `otel.trace_id` and `otel.span_id` tags
- **Cross-Platform Navigation**: Copy trace ID from Sentry issue → paste into Grafana/Tempo for full distributed trace context
- **Smart Filtering**: Health checks and metrics endpoints are excluded from error reporting to reduce noise
- **Enhanced Context**: Combines Sentry's native error tracking with OpenTelemetry's distributed tracing


## Request Lifecycle (Sequence Diagram)

```mermaid
sequenceDiagram
    actor User
    participant App as BookApp Service
    participant DB as PostgreSQL
    participant Kafka as Kafka Topic
    participant Backend as Backend Worker
    participant Sentry
    participant OTelCollector as OpenTelemetry Collector

    User->>+App: POST /books/add
    App->>App: Start Trace (Span 1: Request)
    App->>OTelCollector: Export Span 1

    App->>+DB: INSERT into books (Span 2: DB Write)
    App->>OTelCollector: Export Span 2
    DB-->>-App: Return new book_id

    App->>+Kafka: Produce ingestion message<br/>(Injects Trace Context in Headers)<br/>(Span 3: Kafka Produce)
    App->>OTelCollector: Export Span 3
    Kafka-->>-App: Ack

    App-->>-User: 200 OK (new_id)

%% Asynchronous Processing
    Note over Kafka, Backend: Later, consumer picks up the message...

    Kafka->>+Backend: Consume message
    Backend->>Backend: Extract Trace Context from Headers
    Backend->>Backend: Create *Linked* Span (Span 4: Consume & Process)
    Backend->>OTelCollector: Export Span 4
    Backend-->>-Kafka: Commit offset

%% Error Scenario
    Note over App, Sentry: Later, an error occurs...
    App->>App: An error is triggered
    App->>App: Custom layer adds<br/>otel.trace_id & otel.span_id<br/>to Sentry Scope
    App->>+Sentry: Capture Exception
    Sentry-->>-App: Event ID
```

## Enhanced Tokio Runtime Observability

This project includes comprehensive Tokio async runtime observability through the custom `tokio-otel-metrics` crate, providing deep insights into runtime performance and task scheduling behavior.

### Features

- **15+ Runtime Metrics**: Worker threads, task queues, scheduling patterns, blocking operations
- **OpenTelemetry 0.30+ Compatible**: Full OTLP export using modern observable instrument callbacks
- **Zero-Overhead Collection**: Efficient callback-based observables (no polling or background threads)
- **Worker-Level Granularity**: Per-worker metrics with proper labeling (`worker_id` attributes)
- **Task-Level Tracking**: Spawn counts, active tasks, completion rates, and slow-start detection
- **Grafana Integration**: Pre-built dashboards with real-time visualizations

### Key Metrics Categories

| Category | Metrics | Purpose |
|----------|---------|---------|
| **Runtime Core** | `tokio_workers_count`, `tokio_alive_tasks`, `tokio_global_queue_depth` | Overall runtime health and load |
| **Worker Performance** | `tokio_worker_busy_duration_seconds`, `tokio_worker_poll_count` | Per-worker efficiency and utilization |
| **Task Lifecycle** | `tokio_spawned_tasks_total`, `tokio_task_active_count` | Task creation and completion tracking |
| **Blocking Operations** | `tokio_blocking_threads`, `tokio_blocking_queue_depth` | Blocking task pool monitoring |
| **Scheduling Health** | `tokio_budget_forced_yield_count`, `tokio_remote_schedule_count` | Cooperative scheduling and external coordination |

### Implementation Highlights

- **Observable Instruments**: Uses OpenTelemetry's `.with_callback()` pattern for efficient metric collection
- **Proper Labeling**: Worker-specific metrics include `worker_id` labels for granular analysis
- **Semantic Compliance**: Follows OpenTelemetry semantic conventions for async runtime metrics
- **Runtime Integration**: Initializes automatically in both `bookapp` and `backend` services
- **Dashboard Ready**: Exports to Prometheus with Grafana visualization support

### Accessing Observability

1. **Grafana Dashboard**: http://localhost:3000 → "Tokio Runtime Observability Dashboard"  
2. **Prometheus Metrics**: Query `tokio_*` metrics at http://localhost:3000/explore
3. **Performance Analysis**: See live worker utilization, task spawn rates, and queue depths

### Performance Impact

- **Memory Overhead**: <0.5% additional usage
- **CPU Overhead**: <0.1% additional usage (callback-based collection)
- **Collection Method**: No background polling - metrics collected on-demand by OpenTelemetry

For implementation details, see the `tokio-otel-metrics/` crate and `observability-utils/src/tracing_config.rs`.

## Running locally

```shell
# Optional: Configure Sentry integration
cp .env.example .env
# Edit .env to add your SENTRY_DSN if you want error tracking

docker compose build
docker compose --profile ci up
```

### Docker Compose Profiles

The project uses Docker Compose profiles to support different deployment scenarios:

- **Default profile**: Full application stack (app, backend, database, Kafka, observability, integration tests)
- **CI profile**: Infrastructure for testing (database, Kafka, observability, integration tests)

To run specific profiles:

```shell
# Run full application stackdocker compose up -d

# Run with test container
docker compose --profile ci up

# Run specific services (without profiles)
docker compose up db kafka telemetry
```


## Key Endpoints

```http request
### GET all books
GET http://localhost:8000/books
Accept: application/json

### Authors - list/create
GET http://localhost:8000/authors/
Accept: application/json

POST http://localhost:8000/authors/add
Content-Type: application/json

{"name":"Test Author","sort_name":"Author, Test"}

### Works - create (appends outbox event)
POST http://localhost:8000/works/add
Content-Type: application/json

{"title":"My Work","original_language":"en","publication_year":2024}

### Editions - create
POST http://localhost:8000/editions/add
Content-Type: application/json

{"work_id":1,"isbn":"9780000000000","title":"Edition Title"}

### Series - create and add work
POST http://localhost:8000/series/add
Content-Type: application/json

{"name":"My Series"}

POST http://localhost:8000/series/{id}/works/add
Content-Type: application/json

{"work_id":1,"primary_work":true,"order_id":1}

### Health check
GET http://localhost:8000/health
Accept: application/json

### Error injection configuration
GET http://localhost:8000/error-injection
Accept: application/json
```

Open Grafana at localhost:3000

## Open Library Loader (optional)

A lightweight Open Library subject loader is included to populate the normalized schema and outbox with real data.

- Location: `bookapp-dal/loaders/openlibrary_loader.py`
- Behavior: Idempotent UPSERTs for authors, works, work_authors (primary), and optional editions; appends outbox events (default topic `domain.events`)
- Requirements: `DATABASE_URL` env var and Postgres running

Quick start:

```shell
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
# Optionally override outbox topic
# export OUTBOX_DEFAULT_TOPIC="domain.events"

# Migrate schema if needed
cd bookapp-dal && sqlx migrate run && cd ..

# Run loader with minimal deps via uvx
uvx --with requests --with "psycopg[binary]" \
  python bookapp-dal/loaders/openlibrary_loader.py \
  --subject "science_fiction" --limit 100 --editions-per-work 0 --emit-events
```

Notes:
- Safe to re-run (UPSERTs)
- Events are picked up by the backend outbox publisher and sent to Kafka

![img.png](.github/img.png)


## Data Loader

The included `data-loader` utility demonstrates how to perform bulk operations using the Progenitor client with full OpenTelemetry integration.

### Features
- **Concurrent Workers**: Configurable number of workers for parallel book creation
- **Rate Limiting**: Adjustable delay between requests to control load
- **Auto-Generated Client**: Uses the Progenitor-generated client with built-in tracing
- **Cross-Service Tracing**: Full trace propagation from data-loader through bookapp to backend services
- **Progress Tracking**: Real-time feedback on creation success/failure rates

### Usage Examples

```shell
# Build the data-loader
cargo build --bin data-loader

# Basic usage - create 100 books with default settings
./target/debug/data-loader

# Custom configuration
./target/debug/data-loader \
  --app-url http://localhost:8000 \
  --count 500 \
  --delay-ms 100 \
  --workers 10 \
  --otlp-endpoint http://localhost:4317

# High-throughput testing (minimal delay, more workers)  
./target/debug/data-loader --count 1000 --delay-ms 10 --workers 20
```

### Command Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `--app-url` | `http://localhost:8000` | Bookapp service endpoint |
| `--count` | `100` | Total number of books to create |
| `--delay-ms` | `50` | Delay between requests (milliseconds) |
| `--workers` | `5` | Number of concurrent workers |
| `--otlp-endpoint` | `http://localhost:4317` | OpenTelemetry collector endpoint |

### Observability Integration

The data-loader provides full telemetry integration:
- **Distributed Tracing**: Each book creation generates a complete trace spanning data-loader → bookapp → database → Kafka → backend
- **Worker Attribution**: Traces include worker ID attributes for parallel operation analysis  
- **Performance Metrics**: Request timing and throughput metrics exported to Prometheus
- **Error Tracking**: Failed operations captured with detailed error context

Use Grafana to observe data-loader operations across the full application stack and verify cross-service trace propagation.

## Load Testing


The provided Locust script is also instrumented with OpenTelemetry.

```shell
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
LOCUST_HOST=http://localhost:8000 \
uvx \
  --with 'opentelemetry-sdk' \
  --with "opentelemetry-exporter-otlp-proto-grpc >=1.24.0" \
  --with "opentelemetry-instrumentation-requests==0.46b0" \
  --with "opentelemetry-instrumentation-urllib3==0.46b0" \
  --with "locust" \
  locust -f requests/locustfile.py --headless -u 50 -r 5 -t 1m --stop-timeout 5 --loglevel INFO
```


![img.png](./.github/locust-screenshot.png)

![img.png](./.github/tempo-drilldown.png)

## Data Access Layer (DAL)

The `bookapp-dal` crate implements a clean separation between business logic and data access using the repository pattern with comprehensive features for production applications.

### Architecture & Design

- **Repository Pattern**: Async traits (`AuthorRepository`, `WorkRepository`, etc.) for testability and abstraction
- **Read/Write Pool Separation**: Supports dedicated read replicas and write masters with automatic pool selection
- **Dependency Injection**: Database pools injected from application layer, enabling multiple domain-specific DALs
- **Domain-Driven Design**: Separate repositories for each aggregate root (Author, Work, Edition, Series)
- **Transaction Support**: Explicit transaction management for complex multi-table operations

### Core Features

#### Type Safety & Verification
- **Compile-time SQL Verification**: SQLx macros with prepared query metadata (`.sqlx/` directory)
- **Strong Typing**: Custom types for IDs, enums, and domain objects
- **Null Safety**: Proper Option<T> handling for nullable database columns

#### Advanced Query Capabilities
- **Dynamic Filtering**: Flexible filter parameters with multiple search combinations
- **Full-Text Search**: PostgreSQL tsvector integration for content search
- **Bulk Operations**: Efficient multi-row inserts and updates
- **Complex Joins**: Cross-table queries with proper relationship handling
- **Pagination Support**: Limit/offset with total count queries

#### Error Handling & Observability
- **Custom Error Types**: Structured `DalError` with specific error categories
- **Automatic Instrumentation**: SQLx operations traced via OpenTelemetry
- **Connection Health**: Pool monitoring and connection lifecycle management
- **Query Performance**: Automatic query timing and performance metrics

### Repository Examples

#### Basic CRUD Operations
```rust
use bookapp_dal::{AuthorRepository, AuthorRepositoryImpl, AuthorCreateInput};

// Initialize repository with pool separation
let repo = AuthorRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);

// Create author (write pool)
let author = AuthorCreateInput {
    name: "Brandon Sanderson".to_string(),
    sort_name: "Sanderson, Brandon".to_string(),
};
let author_id = repo.create(author).await?;

// Find by ID (read pool)
let author = repo.find_by_id(author_id).await?;

// Search with patterns (read pool)
let authors = repo.find_by_name_pattern("Sanderson").await?;
```

#### Advanced Filtering
```rust
use bookapp_dal::{WorkRepository, WorkFilterParams};

let repo = WorkRepositoryImpl::new(db_pools.write_pool, db_pools.read_pool);

// Complex filtering with multiple criteria
let params = WorkFilterParams {
    title_pattern: Some("Foundation".to_string()),
    author_name_pattern: Some("Asimov".to_string()),
    language: Some("en".to_string()),
    publication_year_min: Some(1950),
    publication_year_max: Some(1960),
    limit: Some(10),
    offset: Some(0),
};
let works = repo.find_by_filters(params).await?;
```

#### Transaction Management
```rust
use bookapp_dal::{DatabasePools, WorkCreateInput, EditionCreateInput};

// Begin explicit transaction
let mut tx = db_pools.write_pool.begin().await?;

// Create work within transaction
let work_input = WorkCreateInput {
    title: "The Martian".to_string(),
    original_language: "en".to_string(),
    publication_year: Some(2011),
};
let work_id = work_repo.create_with_tx(&mut tx, work_input).await?;

// Create edition within same transaction
let edition_input = EditionCreateInput {
    work_id,
    title: "The Martian".to_string(),
    isbn: Some("9780553418026".to_string()),
};
let edition_id = edition_repo.create_with_tx(&mut tx, edition_input).await?;

// Commit transaction
tx.commit().await?;
```

### Database Pool Configuration

```rust
use bookapp_dal::{DatabasePools, DatabaseConfig};

// Custom pool configuration for high-throughput scenarios
let config = DatabaseConfig {
    max_connections: 100,
    min_connections: 20,
    acquire_timeout: Duration::from_secs(1),
    idle_timeout: Duration::from_secs(30),
    max_lifetime: Duration::from_secs(600),
};

// Initialize with dedicated read/write pools
let db_pools = DatabasePools::new(
    "postgres://user:pass@write-host/db",
    Some("postgres://user:pass@read-host/db"),
    Some(config)
).await?;
```

### Error Handling

The DAL provides comprehensive error handling with specific error types:

```rust
use bookapp_dal::DalError;

match repo.find_by_id(id).await {
    Ok(Some(entity)) => // Found
    Ok(None) => // Not found (not an error)
    Err(DalError::DatabaseConnection(_)) => // Connection issues
    Err(DalError::QueryFailed(_)) => // SQL execution failed
    Err(DalError::InvalidInput(_)) => // Validation error
    Err(DalError::TransactionFailed(_)) => // Transaction rollback
}
```

### Testing Support

The repository pattern enables easy mocking for unit tests:

```rust
#[cfg(test)]
mod tests {
    use mockall::predicate::*;
    use bookapp_dal::MockAuthorRepository;

    #[tokio::test]
    async fn test_author_service() {
        let mut mock_repo = MockAuthorRepository::new();
        mock_repo
            .expect_find_by_id()
            .with(eq(1))
            .times(1)
            .returning(|_| Ok(Some(author_fixture())));
        
        let service = AuthorService::new(Box::new(mock_repo));
        let author = service.get_author(1).await.unwrap();
        assert_eq!(author.name, "Test Author");
    }
}
```

## Database Migrations

The project uses SQLx migrations for schema management, with migrations stored in the DAL crate (`bookapp-dal/migrations/`) but executed by the application layer.

### Automatic Migration

Migrations run automatically when services start:
- **Container Environment**: Migrations execute during `bookapp` container startup
- **Local Development**: Migrations run when starting the application with a valid `DATABASE_URL`

### Manual Migration Management

```shell
# Start database service
docker compose up -d db

# Install sqlx-cli (one-time setup)
cargo install sqlx-cli --no-default-features --features native-tls,postgres

# Set database connection
export DATABASE_URL="postgres://postgres:password@$(docker compose port db 5432)/bookapp"

# Navigate to DAL crate
cd bookapp-dal

# Run pending migrations
sqlx migrate run

# Create new migration
sqlx migrate add <descriptive_migration_name>

# Check migration status
sqlx migrate info

# Revert last migration (use with caution)
sqlx migrate revert
```

### Migration Best Practices

1. **Descriptive Names**: Use clear, descriptive migration names (e.g., `add_book_status_enum`, `create_author_works_junction`)
2. **Idempotent Operations**: Design migrations to be safely re-runnable using `IF NOT EXISTS` clauses
3. **Schema Evolution**: Structure migrations to support zero-downtime deployments when possible
4. **Data Migrations**: Separate schema changes from data migrations for better rollback capability

### Current Schema Overview

The normalized schema supports:
- **Authors**: Author metadata with sort names for proper bibliographic ordering
- **Works**: Conceptual works (can have multiple editions) with publication years and languages  
- **Editions**: Physical/digital editions of works with ISBN identifiers
- **Series**: Named series with ordered work relationships
- **Events Outbox**: Domain events for async processing and cross-service communication

### SQLx Query Preparation

After modifying database queries, update the prepared query metadata:

```shell
cd bookapp-dal
export DATABASE_URL="postgres://postgres:password@$(docker compose port db 5432)/bookapp"

cargo sqlx prepare

# Commit the updated .sqlx/ directory
git add .sqlx && git commit -m "Update SQLx query metadata"
```


## Testing

The project includes comprehensive integration tests that verify end-to-end telemetry functionality:

```shell
# Run unit tests
cargo test --package bookapp

# Run DAL unit tests
cargo test --package bookapp-dal

# Run integration tests (requires running services)
cargo test --package integration-tests

```

Integration tests verify:
- Trace context propagation across HTTP requests
- OpenTelemetry data collection in Tempo, Loki, and Prometheus
- Error injection and telemetry generation
- Cross-service trace correlation
- Repository pattern functionality and database operations
- **Alert validation framework**: End-to-end testing of Grafana alerts to ensure they fire correctly when conditions are met

## Alert Validation Testing

The project includes a comprehensive alert validation framework that tests the entire observability pipeline to ensure alerts work correctly in production.

### Framework Features
- **End-to-end validation**: Triggers alert conditions → Verifies alerts fire → Confirms resolution
- **Threshold verification**: Validates actual metrics exceed expected thresholds via Prometheus
- **Load injection strategies**: Error injection (404s) and latency injection (concurrent requests)  
- **API integration**: Grafana alerts API, Prometheus via proxy, Progenitor client
- **Real-time monitoring**: Configurable timeouts and alert state polling

### Running Alert Tests

```shell
# Test framework connectivity and capabilities
cargo test --package integration-tests --test alert_framework_test

# Full end-to-end alert validation (when alerts aren't already firing)
cargo test --package integration-tests --test alert_validation_test
```

### Validated Alerts
- **Error Ratio Alert**: Monitors 5% error rate threshold with 1-minute duration
- **P95 Latency Alert**: Monitors 500ms SLO threshold with 5-minute duration

The framework provides **continuous confidence** that your observability alerts will fire correctly when real issues occur, validating the entire telemetry pipeline: App → OpenTelemetry → Prometheus → Grafana → Alerting.

For detailed documentation, see [`docs/ALERT_VALIDATION_TESTING.md`](docs/ALERT_VALIDATION_TESTING.md).

