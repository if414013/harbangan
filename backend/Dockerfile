FROM rust:1-slim-bookworm AS builder
RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY . .
RUN cargo build --release --bin kiro-gateway

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/kiro-gateway /app/kiro-gateway
EXPOSE 8000
CMD ["/app/kiro-gateway"]
