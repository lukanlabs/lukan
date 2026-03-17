#!/bin/bash
set -e

# Clean stale PID/lock files from previous runs or host mounts
rm -f ~/.config/lukan/daemon.pid ~/.config/lukan/daemon.lock

# If API key is provided via env, write a minimal config so the daemon starts ready
if [ -n "$ANTHROPIC_API_KEY" ] || [ -n "$OPENAI_API_KEY" ] || [ -n "$OPENAI_COMPATIBLE_API_KEY" ]; then
    echo "API key detected, daemon will use it automatically."
fi

exec lukan "$@"
