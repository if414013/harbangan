FROM rust:1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release --bin kiro-gateway \
    && cp target/release/kiro-gateway /usr/local/bin/kiro-gateway

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl jq && rm -rf /var/lib/apt/lists/* \
    && adduser --disabled-password --gecos '' appuser
WORKDIR /app
COPY --from=builder /usr/local/bin/kiro-gateway /app/kiro-gateway
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh \
    && mkdir -p /data && chown appuser:appuser /data
USER appuser
EXPOSE 8000
CMD ["/app/kiro-gateway"]
