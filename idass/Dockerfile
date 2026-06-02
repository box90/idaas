# ── Build stage ──────────────────────────────────────────────────────────────
FROM rust:1.87-bookworm AS builder

WORKDIR /app

# Build dependencies for native-tls (reqwest, sqlx) and samael (libxml2)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libxml2-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests and lock file first so dependency layers are cached
COPY Cargo.toml Cargo.lock rust-toolchain.toml .cargo/ ./
COPY api-gateway/Cargo.toml  ./api-gateway/
COPY mod-auth/Cargo.toml     ./mod-auth/
COPY mod-token/Cargo.toml    ./mod-token/
COPY mod-mgmt/Cargo.toml     ./mod-mgmt/
COPY shared-kernel/Cargo.toml ./shared-kernel/

# Stub source files so Cargo can resolve and cache all dependencies
RUN mkdir -p api-gateway/src mod-auth/src mod-token/src mod-mgmt/src shared-kernel/src && \
    echo 'fn main() {}' > api-gateway/src/main.rs && \
    echo ''              > api-gateway/src/lib.rs  && \
    touch mod-auth/src/lib.rs mod-token/src/lib.rs \
          mod-mgmt/src/lib.rs shared-kernel/src/lib.rs && \
    SQLX_OFFLINE=true cargo build --release -p api-gateway 2>/dev/null; \
    rm -rf api-gateway/src mod-auth/src mod-token/src mod-mgmt/src shared-kernel/src

# Copy full source (including .sqlx offline cache)
COPY . .

# Build the real binary
RUN SQLX_OFFLINE=true cargo build --release -p api-gateway

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libxml2 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/idass ./idass

EXPOSE 8080

ENTRYPOINT ["./idass"]
