ARG RUST_VERSION=1.78.0

FROM rust:${RUST_VERSION}-slim-bookworm as builder
WORKDIR /usr/src/bookapp
COPY . .
RUN cargo install --path .

FROM debian:12.6-slim
#RUN apt-get update && apt-get install -y extra-runtime-dependencies && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/bookapp /usr/local/bin/bookapp

ENV RUST_LOG="info,sqlx=info,bookapp=debug"
CMD ["bookapp"]
EXPOSE 8000