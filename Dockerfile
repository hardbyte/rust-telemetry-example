ARG RUST_VERSION=1.98

FROM rust:${RUST_VERSION}-trixie AS builder
WORKDIR /usr/src/bookapp
ENV SQLX_OFFLINE=true

# Copy the full source and build the app in release mode
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/bookapp/target \
    cargo build --release --package bookapp --package preaggregated-metrics && \
    cp /usr/src/bookapp/target/release/preaggregated-metrics /preaggregated-metrics && \
    mv /usr/src/bookapp/target/release/bookapp /bookapp


FROM debian:trixie-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*

FROM runtime AS histogram-demo
COPY --from=builder /preaggregated-metrics /usr/local/bin/preaggregated-metrics
CMD ["preaggregated-metrics", "--demo"]

FROM runtime AS bookapp
COPY --from=builder /bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug,backend=debug"
CMD ["bookapp"]
EXPOSE 8000