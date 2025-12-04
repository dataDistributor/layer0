# Multi-stage build
FROM rust:1.72 as builder
WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y pkg-config libssl-dev
RUN cargo build -p dxid-node --release

FROM debian:12-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/dxid-node /usr/local/bin/dxid-node
COPY config/dxid.toml /app/config/dxid.toml
ENV DXID_CONFIG=/app/config/dxid.toml
EXPOSE 8080 50051 7000
ENTRYPOINT ["/usr/local/bin/dxid-node"]
