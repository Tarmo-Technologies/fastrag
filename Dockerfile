# Multi-stage build for fastrag-cli.
# Final image is a distroless static base — no shell, no package manager.

FROM rust:1.82-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p fastrag-cli --no-default-features \
        --features language-detection,retrieval

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /build/target/release/fastrag /usr/local/bin/fastrag
USER nonroot:nonroot
EXPOSE 8081
ENV FASTRAG_LOG=info \
    FASTRAG_LOG_FORMAT=json
ENTRYPOINT ["/usr/local/bin/fastrag"]
CMD ["serve-http", "--corpus", "/var/lib/fastrag/corpus", "--port", "8081"]
