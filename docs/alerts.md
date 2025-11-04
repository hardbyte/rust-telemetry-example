# Alert Validation Guide

This guide explains how the project exercises its Grafana alerts end to end, how to run the suites locally, and how to debug failures.

## Overview

Two focused integration tests live in `tests/src/alert_validation_test.rs`:

| Test | Purpose | Threshold & Window | Provisioned Rule |
| --- | --- | --- | --- |
| `test_error_ratio_alert` | Ensure the temporary error-rate alert fires and resolves | Error rate >5 %, evaluation every 30 s | UID `integration_error_ratio_test-<suffix>` created through the Grafana Ruler API and paused/unpaused during cooldown |
| `test_latency_p95_alert` | Validate P95 latency alert behaviour end to end | P95 latency >500 ms, evaluation every 30 s | UID `integration_latency_p95_test`, deleted during teardown |

Both tests:

- Provision short-lived Grafana rules under the “Integration Tests” folder using `X-Disable-Provenance: true`.
- Talk to Grafana, Prometheus (via the Grafana proxy), and Tempo through the URLs exported below.
- Use the Bookapp Progenitor client to generate traffic and, when needed, toggle the error-injection middleware.

## Quick Start

```bash
# 1. Start the stack – requires docker compose v2.20+
docker compose up -d --wait --wait-timeout 180

# 2. Discover ports (Compose publishes random high ports on each run)
APP_PORT=$(docker compose port app 8000 | awk -F: '{print $2}')
GRAFANA_PORT=$(docker compose port telemetry 3000 | awk -F: '{print $2}')
TEMPO_PORT=$(docker compose port telemetry 3200 | awk -F: '{print $2}')

# 3. Export endpoints used by the tests
export APP_BASE_URL="http://127.0.0.1:${APP_PORT}"
export GRAFANA_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}"
export PROMETHEUS_BASE_URL="${GRAFANA_BASE_URL}/api/datasources/proxy/1"
export TEMPO_BASE_URL="http://127.0.0.1:${TEMPO_PORT}"
export TEMPO_DIRECT_URL="$TEMPO_BASE_URL"
SQLX_OFFLINE=true cargo test --package integration-tests --test alert_validation_test -- --nocapture

# 4. Tear down when finished
docker compose down --remove-orphans
```

Typical runtimes on a developer laptop:

- Error ratio alert: 2–3 minutes (includes pause/unpause cycle while metrics cool down)
- P95 latency alert: 4 minutes (120 s of injected latency plus resolution wait)
- Full module: around 6–7 minutes

## Test Flow Details

### Error ratio alert

1. Clean up existing error-injection configs and send healthy traffic for at least 45 s.
2. Provision a temporary Grafana rule (`integration_error_ratio_test-<timestamp>`) with the expression  
   `(sum(rate(traces_spanmetrics_calls_total{status_code="STATUS_CODE_ERROR", service="bookapp"}[30s])) / clamp_min(sum(rate(traces_spanmetrics_calls_total{service="bookapp"}[30s])), 0.001)) or on() vector(0)`.
3. Inject errors for 90 s by mixing invalid book IDs with regular traffic.
4. Wait for Grafana to report the rule in the `active` state, verify the ratio through Prometheus, then pause the rule.
5. Continue sending healthy traffic while paused, unpause, and wait (up to 4 minutes) for Grafana and the `ALERTS` metric to show the rule resolved.

### P95 latency alert

1. Create the short-lived rule `integration_latency_p95_test` with the expression  
   `histogram_quantile(0.95, sum(rate(traces_spanmetrics_latency_bucket{service="bookapp", span_kind="SPAN_KIND_SERVER"}[1m])) by (le))`.
2. Post a latency config to the error-injection API (`/books/:id`, `latency_ms = 600`) and hammer the endpoint with concurrent requests for 120 s.
3. Once Grafana marks the rule `active`, query Prometheus for the observed P95, remove the injection, and send recovery traffic.
4. Wait (up to 3 minutes) for Grafana to move the rule out of `active`.

## Running the Harness

For the full lint/unit/integration workflow, use:

```bash
SQLX_OFFLINE=true ./run_tests.sh
```

The script now relies on Compose orchestration:

- `docker compose up -d --wait --wait-timeout 240 kafka telemetry app backend`
- Resolves ports and exports `APP_BASE_URL`, `GRAFANA_BASE_URL`, `PROMETHEUS_BASE_URL`, `TEMPO_BASE_URL`, `TEMPO_DIRECT_URL`
- Runs the integration crate with `cargo test --package integration-tests -- --nocapture`

In the `.run_tests/` directory you will find timestamped logs and any failure artefacts.

### Soak / flake hunting

`scripts/integration_loop.sh` repeatedly provisions the stack, runs the integration crate, and stores per-iteration logs under `logs/integration_loop/iter_<n>_*` with a summary in `logs/integration_loop/summary_*.log`.

```bash
INTEGRATION_LOOP_ITERATIONS=30 ./scripts/integration_loop.sh
# or to tweak timeouts:
INTEGRATION_LOOP_TIMEOUT_SECS=1200 ./scripts/integration_loop.sh
```

The script uses `docker compose up -d --wait` and captures the resolved ports before each iteration.

## Troubleshooting

| Symptom | Likely Cause | Suggested Action |
| --- | --- | --- |
| `PROMETHEUS_BASE_URL` 404s | Hitting Prometheus directly | Use Grafana’s proxy: `http://<grafana>/api/datasources/proxy/1` |
| Alert stays `active` after cleanup | Grafana rule still evaluating paused traffic | Ensure the pause/unpause calls succeed (look for 404s) and the cooldown loop finishes |
| No outbox/Kafka traces during worker test | Background jobs haven’t fired yet | The test now retries with exponential backoff; if it still fails, inspect backend logs for scheduler issues |
| Error endpoint returns 404/400 instead of 500 | Error injection pattern didn’t match | The test now retries against a numeric `/books/<id>` path and logs if no 500 arrives—check `bookapp` logs if it keeps failing |
| Tempo requests refuse connection | Tempo container still starting | Compose `--wait` should prevent this; if manual runs fail, query `docker compose logs telemetry` |
| Grafana API 401/403 | Missing auth header | Basic auth is `admin:admin`; the tests send it automatically |

For deeper debugging, tail the following logs:

- Grafana & Tempo: `docker compose logs telemetry`
- Bookapp & backend workers: `docker compose logs app backend`
- Integration tests: `.run_tests/run_tests_<timestamp>.log` or individual `logs/integration_loop/iter_<n>_*` files

If you adjust an alert expression or Grafana configuration, re-run:

```bash
SQLX_OFFLINE=true cargo test --package integration-tests --test alert_validation_test -- --nocapture
```

and confirm both tests still fire and resolve.
