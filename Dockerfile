ARG RUST_VERSION=1.80.1

FROM rust:${RUST_VERSION}-bookworm as builder
WORKDIR /usr/src/bookapp
COPY . .

RUN cargo install --path .

FROM debian:12.6
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug,backend=debug"
CMD ["bookapp"]
EXPOSE 8000