# ── Stage 1: Build edms-child from compute/ ─────────────────────────
FROM rust:1.90-bookworm AS compute-builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /compute
COPY compute/Cargo.toml compute/Cargo.lock ./
COPY compute/src ./src
RUN cargo build --release

# ── Stage 2: Build rust-webserver from webserver/ ───────────────────
FROM rust:1.90-bookworm AS webserver-builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /webserver
COPY compute/Cargo.toml compute/Cargo.lock /compute/
COPY compute/src /compute/src
COPY webserver/Cargo.toml webserver/Cargo.lock ./
COPY webserver/libs ./libs
COPY webserver/src ./src
RUN cargo build --release

# ── Stage 3: Runtime ────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libssl3 \
    libsqlite3-0 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Webserver binary
COPY --from=webserver-builder /webserver/target/release/rust-webserver .

# edms-child — placed where ipc.rs looks for it
COPY --from=compute-builder /compute/target/release/edms-child ./target/release/edms-child

RUN cp webserver/src/config.yml ./src/config.yml 2>/dev/null || true

RUN mkdir -p edms_root/endpoints/reports \
             edms_root/exports \
             edms_root/session-backup \
             edms_root/active \
             edms_root/temp \
             edms_root/repo \
             edms_data

RUN mkdir -p /edms_root && useradd -m appuser && chown -R appuser /app /edms_root
USER appuser

EXPOSE 3000
CMD ["./rust-webserver"]