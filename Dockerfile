# syntax=docker/dockerfile:1

# netcode hub — multi-stage build: compile static musl binaries inside the
# container, ship a FROM-scratch runtime with just the hub, the sample
# worker, and a default config. Building this image requires nothing but
# docker on a clean checkout.

FROM rust:1-slim AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends musl-tools \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --target x86_64-unknown-linux-musl \
    -p hub -p sample-worker -p conformance

FROM scratch
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/hub /hub
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/sample-worker /sample-worker
COPY deploy/hub.toml /hub.toml
EXPOSE 4433/udp 8080/tcp
ENTRYPOINT ["/hub"]
