FROM docker.io/library/rust:bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libfontconfig-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN cargo build --release --bin clise-cli

# --- runtime ---
FROM docker.io/library/debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    libfontconfig1 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/clise-cli /usr/local/bin/clise

ENTRYPOINT ["clise"]
