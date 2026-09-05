# Rust Telemetry & Observability Example

This repository provides a comprehensive example of cross-service telemetry and observability in a distributed Rust application. It demonstrates how to effectively implement the three pillars of observability—traces, metrics, and logs—using OpenTelemetry and the Rust ecosystem.

The project is a multi-service "Book App" designed to showcase how to trace requests as they flow through REST APIs, asynchronous message queues (Kafka), and database interactions.

## Architecture

The system consists of two Rust services, a separate data access layer (DAL), a PostgreSQL database, and a Kafka message queue. All telemetry data is collected by the OpenTelemetry Collector and visualized using Grafana, Loki, Tempo, and Prometheus. Sentry is integrated for advanced error tracking and is correlated with the OpenTelemetry traces.

### Crate Structure

This is a Cargo workspace with the following crates:

- **`bookapp`**: Main REST API service (port 8000)
- **`bookapp-dal`**: Data access layer with repository pattern and SQLx integration
- **`backend`**: Secondary service for cross-service communication (port 8001)  
- **`client`**: Generated API client using Progenitor for type-safe service calls
- **`tests`**: Integration tests for end-to-end telemetry validation


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
  app -- "2. HTTP API Call" --> backend
  app -- "3. Produces Message" --> kafka

  kafka -- "Delivers Message" --> backend
  backend -- "Processes & Reads from DB" --> db

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

For data that is already aggregated, see [Preaggregated OTLP histograms](preaggregated-metrics/README.md).
This standalone Rust example sends supplied bucket populations directly using OpenTelemetry's
generated OTLP client, with a provisioned Grafana dashboard and no scrape endpoint.

In application metrics are implemented via the OpenTelemetry meter, including HTTP request
duration recorded by an Axum middleware. The otel collector also collects metrics from postgresql and
kafka exporting them all to Prometheus.

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

## Running locally

```shell
# Optional: Configure Sentry integration
cp .env.example .env
# Edit .env to add your SENTRY_DSN if you want error tracking

docker compose build
docker compose --profile default up
```

### Docker Compose Profiles

The project uses Docker Compose profiles to support different deployment scenarios:

- **Default profile**: Full application stack (app, backend, database, Kafka, observability, integration tests)
- **CI profile**: Infrastructure for testing (database, Kafka, observability, integration tests)

To run specific profiles:

```shell
# Run full application stack (recommended)
docker compose --profile default up

# Run detached (background)
docker compose --profile default up -d

# Run only infrastructure for development/testing
docker compose --profile ci up

# Run specific services (without profiles)
docker compose up db kafka telemetry
```

**Note**: Due to service profiles, you must specify `--profile default` to run the complete application stack including the main app and backend services.


## Key Endpoints

```http request
### GET all books
GET http://localhost:8000/books
Accept: application/json

### Health check
GET http://localhost:8000/health
Accept: application/json

### Error injection configuration  
GET http://localhost:8000/error-injection
Accept: application/json

```

Open Grafana at [localhost:3000](http://localhost:3000) — anonymous auth is enabled in `docker-compose.yaml`, so no login is needed. Two dashboards are pre-provisioned from `telemetry-config/dashboards/`:

- **Custom Dashboard** (`/d/ae04v0u4pbg8wb/custom-dashboard`): bookapp-specific RED metrics — request rate, error %, p50/p95/p99 latency with trace exemplars, slowest internal spans, service-dependency edges, and a live ERROR/WARN log feed from Loki. Use the `Service` template variable at the top to switch between `bookapp` and `backend`.
- **OpenTelemetry Collector** (`/d/BKf2sowmj/opentelemetry-collector`): receiver/processor/exporter throughput, queue depth, batch size, collector memory. The RPC and Kubernetes rows are pre-collapsed; expand them only if you run otelcol in a multi-collector or k8s topology.

![img.png](.github/img.png)


## Load Testing


The provided Locust script is also instrumented with OpenTelemetry.

```shell
uvx \
  --with 'opentelemetry-sdk==1.44.0' \
  --with "opentelemetry-exporter-otlp-proto-grpc==1.44.0" \
  --with "opentelemetry-instrumentation-requests==0.65b0" \
  --with "opentelemetry-instrumentation-urllib3==0.65b0" \
  --from locust==2.46.4 locust -f requests/locustfile.py
```


![img.png](./.github/locust-screenshot.png)

![img.png](./.github/tempo-drilldown.png)

## Data Access Layer (DAL)

The `bookapp-dal` crate implements a clean separation between business logic and data access using the repository pattern:

### Features
- **Repository Pattern**: Async traits for testability and abstraction
- **Read/Write Pool Separation**: Supports dedicated read replicas and write masters
- **Dependency Injection**: Pools are injected from application layer, enabling multiple domain-specific DALs
- **Compile-time SQL Verification**: SQLx macros ensure type safety
- **Custom Error Types**: Structured error handling with `DalError`
- **Advanced Filtering**: Dynamic queries with multiple filter combinations
- **Bulk Operations**: Efficient multi-row database operations

### Usage Example
```rust
use bookapp_dal::{BookRepository, BookRepositoryImpl, BookCreateInput};

// Initialize with read/write pool separation
let db_pools = DatabasePools::new(
    &write_url,
    Some(&read_url),  // Optional read replica
    None  // Use default config
).await?;

let repo = BookRepositoryImpl::new(
    db_pools.write_pool, 
    db_pools.read_pool
);

// Create a book (uses write pool)
let input = BookCreateInput {
    title: "Rust Programming".to_string(),
    author: "Steve Klabnik".to_string(),
    status: Some(BookStatus::Available),
};
let book_id = repo.create(input).await?;

// Search with filters (uses read pool)
let params = BookFilterParams {
    status: Some(BookStatus::Available),
    author_pattern: Some("Klabnik".to_string()),
    ..Default::default()
};
let books = repo.find_by_filters(params).await?;
```

## Migrations

Migrations are stored in the DAL crate but executed by the application layer. They run automatically when the bookapp container starts:

```shell
# Start database service
docker compose up -d db

# Install sqlx-cli and run migrations
cargo install sqlx-cli --no-default-features --features native-tls,postgres
cd bookapp-dal
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
sqlx migrate run
sqlx migrate add <new migration>
```

## Compile Time Checked Postgres Queries

The `sqlx` crate is used with the `query!` macro to provide compile time checked queries.

If you change the queries, compile the data access layer crate with `DATABASE_URL` set to a valid postgres
connection string. Prepared metadata is stored in the DAL crate's `.sqlx` directory.

```shell
# Prepare queries for the DAL crate
cd bookapp-dal
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
cargo sqlx prepare
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
