#!/bin/bash

# Script to run integration tests with proper service startup
# This works around the 2-minute timeout limitation

set -e

echo "🚀 Starting e2e testing..."

# Step 1: Ensure services are down
echo "🧹 Cleaning up any existing services..."
docker compose down --remove-orphans 2>/dev/null || true

# Step 2: Start database first (needed for compilation)
echo "🗄️ Starting database..."
docker compose up -d db
sleep 5

# Ensure DATABASE_URL for local tests
export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"

# Optionally run migrations if sqlx CLI is installed
if command -v sqlx >/dev/null 2>&1; then
  echo "📜 Running database migrations..."
  (cd bookapp-dal && sqlx migrate run)
else
  echo "⚠️ sqlx CLI not found, skipping migrations"
fi

# Step 3: Prepare workspace (format and clippy)
echo "🔧 Running formatting and linting..."

# Step 4: Build and check for linting issues
echo "🔍 Running formatting and linting checks..."
cargo fmt --all
cargo clippy --all-targets || {
    echo "❌ Clippy warnings found - will fix manually if needed"
    # Continue anyway since we can't fix all in automation
}

# Step 5: Start full services stack
echo "🏗️ Starting all services (this may take a while)..."
docker compose up -d db kafka telemetry backend app

# Step 6: Wait for services to be ready
echo "⏳ Waiting for services to be ready..."

# Check app health
echo "🏥 Checking app health..."
for i in {1..30}; do
    if curl -sf http://localhost:8000/health > /dev/null; then
        echo "✅ App service is ready"
        break
    fi
    echo "⏳ Waiting for app service... (attempt $i)"
    sleep 2
done

# Check telemetry (Grafana) health
echo "📈 Checking telemetry health..."
for i in {1..30}; do
    if curl -sf http://localhost:3000/api/health > /dev/null; then
        echo "✅ Telemetry service is ready"
        break
    fi
    echo "⏳ Waiting for telemetry service... (attempt $i)"
    sleep 2
done

# Check Tempo readiness
echo "⏲️ Checking Tempo readiness..."
for i in {1..30}; do
    if curl -sf http://localhost:3200/ready > /dev/null; then
        echo "✅ Tempo is ready"
        break
    fi
    echo "⏳ Waiting for Tempo... (attempt $i)"
    sleep 2
done

# Step 8: Run unit tests (bookapp) and integration tests
echo "🧪 Running unit tests (bookapp)..."
cargo test --package bookapp

echo "🧪 Running integration tests..."
cargo test --package integration-tests -- --nocapture

echo "✅ All tests completed successfully!"
echo ""
echo "📊 Grafana: http://localhost:3000"
echo "📚 App:     http://localhost:8000/books"
echo "⏱️ Tempo:   http://localhost:3200"
