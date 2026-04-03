# ── Builder ────────────────────────────────────────────────────────────────
FROM rust:1-slim AS builder
WORKDIR /app

# Cache dependencies separately from source so they aren't re-downloaded
# on every code change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src

# Build the real binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── Runtime ────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/ci_music /usr/local/bin/ci_music
EXPOSE 3000
CMD ["ci_music", "serve"]
