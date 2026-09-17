# Build stage
FROM rust:alpine AS builder
WORKDIR /app
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
COPY Cargo.toml ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src
COPY src src
# update timestamp to force rebuild
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM alpine:latest
WORKDIR /app
# We need ca-certificates for reqwest to verify HTTPS connections to Google
RUN apk add --no-cache ca-certificates openssl
COPY --from=builder /app/target/release/fcm_microservice /usr/local/bin/fcm_microservice

ENV PORT=8080
EXPOSE 8080

CMD ["fcm_microservice"]
