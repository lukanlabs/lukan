FROM ubuntu:24.04

LABEL maintainer="Lukan Labs <hello@lukan.ai>"
LABEL description="Lukan AI Agent — ready to run"
LABEL version="0.1.22"

# Avoid interactive prompts during package install
ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    tmux \
    openssl \
    libssl3 \
    sudo \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user with sudo access
RUN useradd -m -s /bin/bash lukan && echo "lukan ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers

# Install Lukan binary from R2
ARG TARGETARCH
RUN ARCH=$([ "$TARGETARCH" = "arm64" ] && echo "arm64" || echo "amd64") && \
    curl -fsSL -o /usr/local/bin/lukan "https://get.lukan.ai/lukan-linux-${ARCH}" && \
    chmod +x /usr/local/bin/lukan

# Add entrypoint script
COPY --chmod=755 docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

# Switch to non-root user
USER lukan
WORKDIR /home/lukan

# Create config directory
RUN mkdir -p /home/lukan/.config/lukan

# Default port for web UI / daemon
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -sf http://localhost:3000/health || exit 1

# Entrypoint cleans stale PID files, then runs lukan
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["daemon", "start"]
