# Multi-stage build for extractous API
FROM rust:1.82-bookworm AS builder

# Install system dependencies for building
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libtesseract-dev \
    tesseract-ocr \
    openjdk-17-jdk \
    && rm -rf /var/lib/apt/lists/*

# Set JAVA_HOME for the build
ENV JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64

WORKDIR /app

# Copy and build the application
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build the application
RUN cargo build --release

# Find and collect native libraries created during build
RUN find /usr/local/cargo/registry -name "*.so*" -exec cp {} /tmp/ \; 2>/dev/null || true && \
    find target -name "*.so*" -exec cp {} /tmp/ \; 2>/dev/null || true && \
    find /root/.cargo -name "*.so*" -exec cp {} /tmp/ \; 2>/dev/null || true && \
    ls -la /tmp/*.so* || echo "No .so files found"

# Runtime stage
FROM debian:bookworm-slim

# Accept build argument for tesseract languages
ARG TESSERACT_LANGUAGES=eng+rus

# Install base runtime dependencies and dynamically install tesseract language packages
RUN apt-get update && \
    # Install base packages
    apt-get install -y \
        tesseract-ocr \
        libssl3 \
        ca-certificates \
        openjdk-17-jre-headless \
        curl && \
    # Parse TESSERACT_LANGUAGES and install language packages
    echo "$TESSERACT_LANGUAGES" | tr '+' '\n' | while read lang; do \
        if [ -n "$lang" ]; then \
            echo "Installing tesseract-ocr-$lang"; \
            apt-get install -y tesseract-ocr-$lang || echo "Warning: tesseract-ocr-$lang not found"; \
        fi; \
    done && \
    rm -rf /var/lib/apt/lists/*

# Set environment variables
ENV JAVA_HOME=/usr/lib/jvm/java-17-openjdk-amd64
ENV LD_LIBRARY_PATH=$JAVA_HOME/lib/server:$JAVA_HOME/lib
ENV RUST_LOG=info
ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=8080
ENV TESSERACT_LANGUAGES=$TESSERACT_LANGUAGES

# Create non-root user
RUN useradd --system --create-home --shell /bin/false extractous

WORKDIR /app

# Copy the built binary
COPY --from=builder /app/target/release/simple_extractous_api ./extractous-api

# Copy native libraries if they exist
RUN --mount=from=builder,source=/tmp,target=/tmp-builder \
    cp /tmp-builder/*.so* /usr/local/lib/ 2>/dev/null || echo "No native libraries found"

# Create temp directory and set permissions
RUN mkdir -p /app/temp_uploads && \
    chown -R extractous:extractous /app && \
    ldconfig

# Switch to non-root user
USER extractous

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
CMD ["./extractous-api"]