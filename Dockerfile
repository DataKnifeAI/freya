FROM rust:1.88-bookworm AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY packs ./packs
RUN cargo build --release -p freya-operator -p freya-api -p freya-worker -p freya-mcp

FROM gcr.io/distroless/cc-debian12:nonroot AS operator
COPY --from=builder /src/target/release/freya-operator /usr/local/bin/freya-operator
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/freya-operator"]

# debian slim (not distroless) so gif modality can run ffmpeg.
# UID 65532 matches the previous distroless nonroot and gallery PVC ownership.
FROM debian:bookworm-slim AS api
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 nonroot \
    && useradd --uid 65532 --gid 65532 --no-create-home --shell /usr/sbin/nologin nonroot
COPY --from=builder /src/target/release/freya-api /usr/local/bin/freya-api
COPY packs /packs
USER 65532:65532
ENV FREYA_PACKS_DIR=/packs
ENTRYPOINT ["/usr/local/bin/freya-api"]

FROM gcr.io/distroless/cc-debian12:nonroot AS worker
COPY --from=builder /src/target/release/freya-worker /usr/local/bin/freya-worker
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/freya-worker"]

FROM gcr.io/distroless/cc-debian12:nonroot AS mcp
COPY --from=builder /src/target/release/freya-mcp /usr/local/bin/freya-mcp
USER nonroot:nonroot
ENV FREYA_MCP_BIND=0.0.0.0:8090
ENTRYPOINT ["/usr/local/bin/freya-mcp"]
