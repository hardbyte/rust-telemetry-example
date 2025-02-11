ARG RUST_VERSION=1.84.0

FROM rust:${RUST_VERSION}-bookworm AS builder
WORKDIR /usr/src/bookapp
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo install --path bookapp

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug,backend=debug"
CMD ["bookapp"]
EXPOSE 8000