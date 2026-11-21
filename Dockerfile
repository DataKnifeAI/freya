FROM rust:1.87-bookworm AS builder
WORKDIR /src
COPY Cargo.toml rust-toolchain.toml ./
COPY crates ./crates
COPY packs ./packs
RUN cargo build --release -p freya-operator -p freya-api -p freya-worker

FROM gcr.io/distroless/cc-debian12:nonroot AS operator
COPY --from=builder /src/target/release/freya-operator /usr/local/bin/freya-operator
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/freya-operator"]

FROM gcr.io/distroless/cc-debian12:nonroot AS api
COPY --from=builder /src/target/release/freya-api /usr/local/bin/freya-api
COPY packs /packs
USER nonroot:nonroot
ENV FREYA_PACKS_DIR=/packs
ENTRYPOINT ["/usr/local/bin/freya-api"]

FROM gcr.io/distroless/cc-debian12:nonroot AS worker
COPY --from=builder /src/target/release/freya-worker /usr/local/bin/freya-worker
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/freya-worker"]
