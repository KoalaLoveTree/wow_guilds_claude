# Multi-stage Dockerfile for WoW Guild Bot
# Optimized for small size, fast builds, and Dokku deployment

# Build stage
FROM rust:slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user and directory
RUN useradd -m -u 1001 appuser
WORKDIR /app

# Copy dependency files first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached unless dependencies change)
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src

# Build the application
# Remove the dummy target and rebuild with actual source
RUN rm target/release/deps/wow_guild_bot* && \
    cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create app user
RUN useradd -m -u 1001 appuser

# Create app directory and set permissions
WORKDIR /app
RUN chown appuser:appuser /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/wow_guild_bot /app/wow_guild_bot
RUN chmod +x /app/wow_guild_bot

# Create data directory for SQLite database and files
RUN mkdir -p /app/data && chown appuser:appuser /app/data

# Switch to non-root user
USER appuser

# Expose port (Dokku will set PORT environment variable)
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD [ -f /app/data/wow_guild_bot.db ] || exit 1

# Default command
CMD ["/app/wow_guild_bot"]
