# MSRV: transitive crates (e.g. ort, zip, darling) require rustc ≥ 1.88.
# ort-sys links prebuilt ONNX Runtime objects that reference glibc ≥ 2.38 (__isoc23_*);
# Bookworm’s glibc is too old — use trixie for link + runtime compatibility.
FROM rust:1.88-trixie AS build

WORKDIR /app

# Native deps: bindgen needs libclang; llama-cpp-sys runs cmake + optional rustfmt on generated code.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        cmake \
        libclang-dev \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY docker ./docker
# Path dependency (see Cargo.toml); helix-db also pulls helix-macros + metrics via relative paths.
COPY vendor/helix ./vendor/helix

RUN cargo build --release --bin ctx --bin ctx-server

FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libgcc-s1 \
        libgomp1 \
        libssl3 \
        libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/ctx-server /usr/local/bin/ctx-server
COPY --from=build /app/target/release/ctx /usr/local/bin/ctx
RUN chmod +x /usr/local/bin/ctx-server /usr/local/bin/ctx

COPY docker/entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENV CTX_HOST=0.0.0.0
ENV CTX_PORT=8080
ENV PORT=8080
ENV CTX_PATH=/mnt/ctx-contexts

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8080/status || exit 1

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["ctx-server", "--port", "8080"]
