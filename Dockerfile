ARG RUST_VERSION=1.84.0

FROM rust:${RUST_VERSION}-bookworm AS builder
WORKDIR /usr/src/bookapp
ENV SQLX_OFFLINE=true

#RUN cargo install --path bookapp

# Copy the full source and build the app in release mode
COPY . .

# Should be able to use cache mounts for the build:
# --mount=type=cache,target=/usr/local/cargo/registry \
# --mount=type=cache,target=/usr/src/bookapp/target \

RUN cargo build --release --package bookapp


FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
#COPY --from=builder /usr/local/cargo/bin/bookapp /usr/local/bin/bookapp
COPY --from=builder /usr/src/bookapp/target/release/bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug,backend=debug"
CMD ["bookapp"]
EXPOSE 8000