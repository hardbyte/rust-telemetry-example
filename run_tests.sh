#!/bin/bash
# Comprehensive test runner that prepares the environment, runs lint/unit/integration
# suites, and (optionally) emits OpenTelemetry spans for each logical stage.
# - Set RUN_TESTS_SKIP_INTEGRATION=1 to short-circuit the long alert validation suite.
# - Set RUN_TESTS_KEEP_STACK=1 to leave Docker services running after completion.
# - Telemetry options:
#     * RUN_TESTS_OTEL_ENDPOINT=auto   -> detect Docker's OTLP port and emit spans.
#     * RUN_TESTS_OTEL_PROTOCOL=http   -> send spans via OTLP/HTTP (defaults to gRPC).
#     * RUN_TESTS_OTEL_DEBUG=1         -> print a confirmation line for every emitted span.
#     * RUN_TESTS_DISABLE_OTEL=1       -> force-disable span emission.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
LOG_DIR=${RUN_TESTS_LOG_DIR:-"$SCRIPT_DIR/.run_tests"}
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/run_tests_$(date +%Y%m%d_%H%M%S).log"
# Mirror output to both stdout and the timestamped log file.
echo "📝 Logs: $LOG_FILE"
exec > >(tee "$LOG_FILE") 2>&1

KEEP_STACK=${RUN_TESTS_KEEP_STACK:-0}
if [[ "$KEEP_STACK" == "1" ]]; then
  echo "ℹ️  RUN_TESTS_KEEP_STACK=1 -> Docker services will remain running after completion"
else
  trap 'docker compose down --remove-orphans 2>/dev/null || true' EXIT
fi

# Shared environment for SQLx compile-time checks
export SQLX_OFFLINE=${SQLX_OFFLINE:-true}
LOOPBACK=${RUN_TESTS_LOOPBACK:-127.0.0.1}
OTEL_AUTO=${RUN_TESTS_OTEL_AUTO:-0}
RUN_TESTS_OTEL_ENDPOINT=${RUN_TESTS_OTEL_ENDPOINT:-${OTEL_EXPORTER_OTLP_ENDPOINT:-}}
if [[ "$RUN_TESTS_OTEL_ENDPOINT" == "auto" ]]; then
  OTEL_AUTO=1
  RUN_TESTS_OTEL_ENDPOINT=""
fi
RUN_TESTS_OTEL_SERVICE=${RUN_TESTS_OTEL_SERVICE:-run-tests}
RUN_TESTS_OTEL_PROTOCOL=${RUN_TESTS_OTEL_PROTOCOL:-grpc}
RUN_TESTS_OTEL_DEBUG=${RUN_TESTS_OTEL_DEBUG:-0}
RUN_TESTS_OTEL_ENABLED=0
OTEL_CLI_BIN=""
PENDING_SPANS=()

if ! command -v timeout >/dev/null 2>&1; then
  echo "❌ The GNU timeout command is required" >&2
  exit 1
fi

flush_pending_spans() {
  [[ $RUN_TESTS_OTEL_ENABLED -eq 1 ]] || return 0
  local record
  for record in "${PENDING_SPANS[@]}"; do
    IFS='|' read -r name start_ns end_ns timeout_secs status command_label <<<"$record"
    emit_span "$name" "$start_ns" "$end_ns" "$timeout_secs" "$status" "$command_label"
  done
  PENDING_SPANS=()
}

ensure_otel_cli() {
  if [[ ${RUN_TESTS_DISABLE_OTEL:-0} == "1" || -z "${RUN_TESTS_OTEL_ENDPOINT}" ]]; then
    return
  fi

  if command -v otel-cli >/dev/null 2>&1; then
    OTEL_CLI_BIN=$(command -v otel-cli)
    RUN_TESTS_OTEL_ENABLED=1
    echo "📡  OpenTelemetry spans -> ${RUN_TESTS_OTEL_ENDPOINT} (${RUN_TESTS_OTEL_PROTOCOL})"
    flush_pending_spans
    return
  fi

  local tools_dir="$SCRIPT_DIR/.tools"
  local bundled_bin="$tools_dir/otel-cli"
  if [[ -x "$bundled_bin" ]]; then
    OTEL_CLI_BIN="$bundled_bin"
    RUN_TESTS_OTEL_ENABLED=1
    echo "📡  OpenTelemetry spans -> ${RUN_TESTS_OTEL_ENDPOINT} (${RUN_TESTS_OTEL_PROTOCOL})"
    flush_pending_spans
    return
  fi

  mkdir -p "$tools_dir"
  local version=${RUN_TESTS_OTELCLI_VERSION:-0.4.5}
  local arch=$(uname -m)
  case "$arch" in
    x86_64|amd64) arch="amd64" ;;
    aarch64|arm64) arch="arm64" ;;
    *) echo "⚠️  Unsupported architecture for otel-cli ($arch); telemetry disabled" >&2
       return ;;
  esac

  local os=$(uname | tr '[:upper:]' '[:lower:]')
  local url="https://github.com/equinix-labs/otel-cli/releases/download/v${version}/otel-cli_${version}_${os}_${arch}.tar.gz"
  local tmpdir
  tmpdir=$(mktemp -d)
  if ! curl -sSL "$url" -o "$tmpdir/otel-cli.tar.gz"; then
    echo "⚠️  Failed to download otel-cli from $url; telemetry disabled" >&2
    rm -rf "$tmpdir"
    return
  fi
  if ! tar -xzf "$tmpdir/otel-cli.tar.gz" -C "$tmpdir" otel-cli >/dev/null 2>&1; then
    echo "⚠️  Failed to extract otel-cli archive; telemetry disabled" >&2
    rm -rf "$tmpdir"
    return
  fi
  mv "$tmpdir/otel-cli" "$bundled_bin"
  chmod +x "$bundled_bin"
  rm -rf "$tmpdir"
  OTEL_CLI_BIN="$bundled_bin"
  RUN_TESTS_OTEL_ENABLED=1
  echo "📡  OpenTelemetry spans -> ${RUN_TESTS_OTEL_ENDPOINT} (${RUN_TESTS_OTEL_PROTOCOL})"
  flush_pending_spans
}

log_section() {
  echo
  echo "==================== $1 ===================="
}

ns_to_epoch() {
  local ns=$1
  local seconds=$((ns / 1000000000))
  local nanos=$((ns % 1000000000))
  printf "%d.%09d" "$seconds" "$nanos"
}

emit_span() {
  local name=$1
  local start_ns=$2
  local end_ns=$3
  local timeout_secs=$4
  local status=$5
  local command_label=$6

  [[ $RUN_TESTS_OTEL_ENABLED -eq 1 ]] || return 0

  local start_epoch end_epoch status_code duration_ms attrs command_attr
  start_epoch=$(ns_to_epoch "$start_ns")
  end_epoch=$(ns_to_epoch "$end_ns")
  duration_ms=$(( (end_ns - start_ns) / 1000000 ))
  status_code=$([[ $status -eq 0 ]] && echo "ok" || echo "error")
  command_attr=$(printf '%s' "$command_label" | tr -cs '[:alnum:].-_' '_')
  attrs="process.exit.code=${status},run_tests.step.timeout=${timeout_secs}"
  if [[ -n "$command_attr" ]]; then
    attrs="${attrs},process.command=${command_attr},run_tests.step.command=${command_attr}"
  fi

  local protocol_args=()
  if [[ -n "$RUN_TESTS_OTEL_PROTOCOL" ]]; then
    protocol_args+=(--protocol "$RUN_TESTS_OTEL_PROTOCOL")
  fi

  if ! "$OTEL_CLI_BIN" span \
      --service "$RUN_TESTS_OTEL_SERVICE" \
      --name "$name" \
      --kind internal \
      --endpoint "$RUN_TESTS_OTEL_ENDPOINT" \
      --start "$start_epoch" \
      --end "$end_epoch" \
      --status-code "$status_code" \
      --attrs "$attrs" \
      "${protocol_args[@]}" >/dev/null 2>&1; then
    echo "⚠️  Failed to emit span for step '$name'" >&2
  elif [[ $RUN_TESTS_OTEL_DEBUG -eq 1 ]]; then
    echo "🛰️  Emitted span: name='${name}' command='${command_attr}' status=${status} duration_ms=${duration_ms}" >&2
  fi
}

record_span() {
  local name=$1
  local start_ns=$2
  local end_ns=$3
  local timeout_secs=$4
  local status=$5
  local command_label=$6

  if [[ $RUN_TESTS_OTEL_ENABLED -eq 1 ]]; then
    emit_span "$name" "$start_ns" "$end_ns" "$timeout_secs" "$status" "$command_label"
  else
    PENDING_SPANS+=("${name}|${start_ns}|${end_ns}|${timeout_secs}|${status}|${command_label}")
  fi
}

run_step() {
  local name=$1
  local timeout_secs=$2
  shift 2
  echo "▶️  $name (timeout ${timeout_secs}s)"
  local start_ns end_ns status
  start_ns=$(date +%s%N)
  if timeout --preserve-status --signal=SIGINT --kill-after=10 "${timeout_secs}s" "$@"; then
    status=0
  else
    status=$?
  fi
  end_ns=$(date +%s%N)
  local command_label
  command_label=$1
  record_span "$name" "$start_ns" "$end_ns" "$timeout_secs" "$status" "$command_label"

  if [[ $status -eq 0 ]]; then
    echo "✅ $name"
  else
    echo "❌ $name failed (exit $status)" >&2
    exit $status
  fi
}

# If the endpoint is already known (not auto-detected), enable telemetry upfront.
if [[ $OTEL_AUTO -ne 1 && -n "$RUN_TESTS_OTEL_ENDPOINT" ]]; then
  ensure_otel_cli
fi

log_section "Bootstrap"
echo "🧹 Cleaning up any existing services..."
docker compose down --remove-orphans 2>/dev/null || true

run_step "Start database" 120 docker compose up -d db
wait_for_db 60 2

DB_PORT=$(docker compose port db 5432 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')
if [[ -z "${DB_PORT}" ]]; then
  echo "❌ Unable to resolve mapped Postgres port" >&2
  exit 1
fi
export DATABASE_URL="postgres://postgres:password@localhost:${DB_PORT}/bookapp"
printenv DATABASE_URL | sed 's/.*/📡 &/'

if command -v sqlx >/dev/null 2>&1; then
  run_step "Apply database migrations" 180 bash -lc 'cd bookapp-dal && sqlx migrate run'
  run_step "Refresh SQLx metadata" 300 bash -lc 'cd bookapp-dal && cargo sqlx prepare -- --all-targets --all-features'
else
  echo "⚠️ sqlx CLI not found; skipping migrations and metadata refresh"
fi

echo "🔧 Setting default SQLX_OFFLINE=${SQLX_OFFLINE}"

log_section "Lint & Unit Tests"
run_step "cargo fmt" 120 cargo fmt --all
run_step "cargo clippy" 600 cargo clippy --all-targets --all-features
run_step "bookapp tests" 600 cargo test --package bookapp
run_step "bookapp-dal tests" 600 cargo test --package bookapp-dal
run_step "backend tests" 600 cargo test --package backend

log_section "Service Stack"
run_step "Start supporting services" 240 docker compose up -d --wait --wait-timeout 240 kafka telemetry app backend

APP_PORT=$(docker compose port app 8000 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')
GRAFANA_PORT=$(docker compose port telemetry 3000 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')
TEMPO_PORT=$(docker compose port telemetry 3200 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')
OTLP_GRPC_PORT=$(docker compose port telemetry 4317 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')

if [[ $OTEL_AUTO -eq 1 && -n "$OTLP_GRPC_PORT" ]]; then
  if [[ "$RUN_TESTS_OTEL_PROTOCOL" == http* ]]; then
    OTLP_HTTP_PORT=$(docker compose port telemetry 4318 2>/dev/null | head -n 1 | awk -F: '{print $2}' | tr -d '\r')
    if [[ -n "$OTLP_HTTP_PORT" ]]; then
      RUN_TESTS_OTEL_ENDPOINT="http://${LOOPBACK}:${OTLP_HTTP_PORT}"
    else
      RUN_TESTS_OTEL_ENDPOINT="http://${LOOPBACK}:${OTLP_GRPC_PORT}"
      RUN_TESTS_OTEL_PROTOCOL="http/protobuf"
    fi
  else
    RUN_TESTS_OTEL_ENDPOINT="${LOOPBACK}:${OTLP_GRPC_PORT}"
  fi
  ensure_otel_cli
fi

if [[ -z "$APP_PORT" || -z "$GRAFANA_PORT" || -z "$TEMPO_PORT" ]]; then
  echo "❌ Failed to resolve required port mappings" >&2
  docker compose ps
  exit 1
fi

printf '📍 App port: %s\n' "$APP_PORT"
printf '📍 Grafana port: %s\n' "$GRAFANA_PORT"
printf '📍 Tempo port: %s\n' "$TEMPO_PORT"

curl -4sf "http://${LOOPBACK}:${APP_PORT}/health" >/dev/null || {
  echo "❌ App service health check failed" >&2
  docker compose logs --tail 50 app || true
  exit 1
}
curl -4sf "http://${LOOPBACK}:${GRAFANA_PORT}/api/health" >/dev/null || {
  echo "❌ Grafana health check failed" >&2
  docker compose logs --tail 50 telemetry || true
  exit 1
}
curl -4sf "http://${LOOPBACK}:${TEMPO_PORT}/ready" >/dev/null || {
  echo "❌ Tempo health check failed" >&2
  docker compose logs --tail 50 telemetry || true
  exit 1
}

export APP_BASE_URL="http://${LOOPBACK}:${APP_PORT}"
export GRAFANA_BASE_URL="http://${LOOPBACK}:${GRAFANA_PORT}"
export TELEMETRY_BASE_URL="http://${LOOPBACK}:${GRAFANA_PORT}"
export PROMETHEUS_BASE_URL="http://${LOOPBACK}:${GRAFANA_PORT}/api/datasources/proxy/1"
export TEMPO_DIRECT_URL="http://${LOOPBACK}:${TEMPO_PORT}"
printf '🌐 APP_BASE_URL=%s\n' "$APP_BASE_URL"
printf '🌐 GRAFANA_BASE_URL=%s\n' "$GRAFANA_BASE_URL"
printf '🌐 TELEMETRY_BASE_URL=%s\n' "$TELEMETRY_BASE_URL"
printf '🌐 PROMETHEUS_BASE_URL=%s\n' "$PROMETHEUS_BASE_URL"
printf '⏱️ Tempo Direct: %s\n' "$TEMPO_DIRECT_URL"

log_section "Integration Tests"
if [[ ${RUN_TESTS_SKIP_INTEGRATION:-0} == "1" ]]; then
  echo "⚠️  Skipping integration tests (RUN_TESTS_SKIP_INTEGRATION=1)"
else
  run_step "integration-tests crate" 900 cargo test --package integration-tests -- --nocapture
fi

echo
echo "✅ All tests completed successfully"
printf '📊 Grafana: %s\n' "$GRAFANA_BASE_URL"
printf '📚 App:     %s/books\n' "$APP_BASE_URL"
printf '⏱️ Tempo:   http://%s:%s\n' "$LOOPBACK" "$TEMPO_PORT"
