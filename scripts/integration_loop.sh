#!/usr/bin/env bash
# Repeatedly run the integration test crate (telemetry + alert validation) to shake out flakes.
# Usage: INTEGRATION_LOOP_ITERATIONS=30 ./integration_loop.sh
# Requires docker compose v2.20+ for `--wait`.
# Logs land in logs/integration_loop/<iteration> with per-step artifacts and a summary file.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ITERATIONS=${INTEGRATION_LOOP_ITERATIONS:-30}
GROUP_SIZE=${INTEGRATION_LOOP_GROUP_SIZE:-10}
STEP_TIMEOUT=${INTEGRATION_LOOP_TIMEOUT_SECS:-1800}
SLEEP_BETWEEN_ITER=${INTEGRATION_LOOP_SLEEP_SECS:-5}
LOG_ROOT=${INTEGRATION_LOOP_LOG_DIR:-"$SCRIPT_DIR/logs/integration_loop"}
mkdir -p "$LOG_ROOT"
SUMMARY_FILE="$LOG_ROOT/summary_$(date +%Y%m%d_%H%M%S).log"
BRINGUP_RETRIES=${INTEGRATION_LOOP_BRINGUP_RETRIES:-3}
BRINGUP_RETRY_DELAY=${INTEGRATION_LOOP_BRINGUP_RETRY_DELAY_SECS:-10}
WAIT_TIMEOUT=${INTEGRATION_LOOP_WAIT_TIMEOUT_SECS:-300}

cleanup_stack() {
  docker compose down --remove-orphans >/dev/null 2>&1 || true
}

bring_up_stack() {
  : > "$ITER_DIR/01_docker_up.log"
  for ((attempt=1; attempt<=BRINGUP_RETRIES; attempt++)); do
    if docker compose up -d --wait --wait-timeout "$WAIT_TIMEOUT" >>"$ITER_DIR/01_docker_up.log" 2>&1; then
      return 0
    fi

    echo "⚠️  docker compose up failed (attempt ${attempt}/${BRINGUP_RETRIES})" | tee -a "$SUMMARY_FILE"
    docker compose logs --tail=200 kafka telemetry app backend >>"$ITER_DIR/01_docker_up.log" 2>&1 || true
    cleanup_stack
    if (( attempt < BRINGUP_RETRIES )); then
      sleep "$BRINGUP_RETRY_DELAY"
    fi
  done

  return 1
}

trap cleanup_stack EXIT

echo "🌀 Integration loop: ${ITERATIONS} iteration(s) (group size ${GROUP_SIZE})" | tee "$SUMMARY_FILE"

total_pass=0
failed_iters=()

for ((i=1; i<=ITERATIONS; i++)); do
  iter_stamp=$(date +%Y%m%d_%H%M%S)
  ITER_DIR="$LOG_ROOT/iter_${i}_$iter_stamp"
  ITER_LOG="$ITER_DIR"
  mkdir -p "$ITER_DIR"

  echo "\n==================== Iteration $i / $ITERATIONS ====================" | tee -a "$SUMMARY_FILE"

  {
    echo "Timestamp: $(date)"
    echo "Logs: $ITER_DIR"
  } > "$ITER_DIR/00_meta.log"

  echo "🧹 [iter $i] docker compose down" | tee -a "$SUMMARY_FILE"
  cleanup_stack

  echo "🏗️ [iter $i] docker compose up -d db kafka telemetry app backend" | tee -a "$SUMMARY_FILE"
  if ! bring_up_stack; then
    echo "❌ docker compose up failed after ${BRINGUP_RETRIES} attempts" | tee -a "$SUMMARY_FILE"
    failed_iters+=($i)
    continue
  fi

  # resolve ports
  APP_PORT=$(docker compose port app 8000 2>/dev/null | awk -F: '{print $2}' | tr -d '\r')
  GRAFANA_PORT=$(docker compose port telemetry 3000 2>/dev/null | awk -F: '{print $2}' | tr -d '\r')
  TEMPO_PORT=$(docker compose port telemetry 3200 2>/dev/null | awk -F: '{print $2}' | tr -d '\r')
  echo "APP_PORT=$APP_PORT" > "$ITER_DIR/02_ports.log"
  echo "GRAFANA_PORT=$GRAFANA_PORT" >> "$ITER_DIR/02_ports.log"
  echo "TEMPO_PORT=$TEMPO_PORT" >> "$ITER_DIR/02_ports.log"

  if [[ -z "$APP_PORT" || -z "$GRAFANA_PORT" ]]; then
    echo "❌ Failed to resolve required ports" | tee -a "$SUMMARY_FILE"
    failed_iters+=($i)
    continue
  fi

  mkdir -p "$ITER_DIR/05_env"
  TEMPO_DIRECT_URL="http://127.0.0.1:${TEMPO_PORT}"

  cat <<ENV >"$ITER_DIR/05_env/export.env"
SQLX_OFFLINE=true
APP_BASE_URL="http://127.0.0.1:${APP_PORT}"
GRAFANA_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}"
PROMETHEUS_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}/api/datasources/proxy/1"
TELEMETRY_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}"
TEMPO_BASE_URL="$TEMPO_DIRECT_URL"
TEMPO_DIRECT_URL="$TEMPO_DIRECT_URL"
ENV

  export SQLX_OFFLINE=true
  export APP_BASE_URL="http://127.0.0.1:${APP_PORT}"
  export GRAFANA_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}"
  export PROMETHEUS_BASE_URL="http://127.0.0.1:${GRAFANA_PORT}/api/datasources/proxy/1"
  export TELEMETRY_BASE_URL="$GRAFANA_BASE_URL"
  export TEMPO_BASE_URL="$TEMPO_DIRECT_URL"
  export TEMPO_DIRECT_URL

  TEST_LOG="$ITER_DIR/10_integration_test.log"
  echo "🧪 [iter $i] running cargo test -p integration-tests" | tee -a "$SUMMARY_FILE"

  if timeout --preserve-status --signal=SIGINT --kill-after=15 "${STEP_TIMEOUT}s" \
    bash -lc 'SQLX_OFFLINE=true cargo test --package integration-tests -- --nocapture' \
    > "$TEST_LOG" 2>&1; then
    echo "✅ Iteration $i succeeded" | tee -a "$SUMMARY_FILE"
    total_pass=$((total_pass+1))
  else
    status=$?
    echo "❌ Iteration $i failed (exit $status)" | tee -a "$SUMMARY_FILE"
    failed_iters+=($i)
  fi

  echo "🪵 [iter $i] capturing docker logs" | tee -a "$SUMMARY_FILE"
  docker compose logs --tail=200 app backend telemetry > "$ITER_DIR/20_docker_logs.log" 2>&1 || true

  if (( i % GROUP_SIZE == 0 )); then
    docker compose logs --tail=500 > "$ITER_DIR/group_${i}_compose.log" 2>&1 || true
  fi

  cleanup_stack
  sleep "$SLEEP_BETWEEN_ITER"
done

echo "\n==================== Summary ====================" | tee -a "$SUMMARY_FILE"
echo "Pass: $total_pass / $ITERATIONS" | tee -a "$SUMMARY_FILE"
if ((${#failed_iters[@]})); then
  echo "Failed iterations: ${failed_iters[*]}" | tee -a "$SUMMARY_FILE"
else
  echo "All iterations passed" | tee -a "$SUMMARY_FILE"
fi

echo "Logs are in $LOG_ROOT" | tee -a "$SUMMARY_FILE"
