ARG RUST_VERSION=1.84

FROM rust:${RUST_VERSION}-bookworm AS builder
WORKDIR /usr/src/bookapp
ENV SQLX_OFFLINE=true

# Copy the full source and build the app in release mode
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/src/bookapp/target \
    cargo build --release --package bookapp && \
    mv /usr/src/bookapp/target/release/bookapp /bookapp


FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*

COPY --from=builder /bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug,backend=debug"
CMD ["bookapp"]
EXPOSE 8000