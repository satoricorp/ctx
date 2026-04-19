FROM rust:1.86-bookworm AS build

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY docker ./docker

RUN cargo build --release --bin ctx --bin ctx-server

FROM debian:bookworm-slim

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
