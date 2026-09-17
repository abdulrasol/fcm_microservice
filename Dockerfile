# Build stage
FROM rust:slim-bookworm AS builder
WORKDIR /app

# Install OpenSSL and pkg-config required by dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Use sparse registry for faster and lighter index fetching
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

RUN cargo build --release
RUN rm -rf src

COPY src src
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim
WORKDIR /app

# Install ca-certificates for HTTPS
RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/fcm_microservice /usr/local/bin/fcm_microservice

ENV PORT=8080
EXPOSE 8080

CMD ["fcm_microservice"]
