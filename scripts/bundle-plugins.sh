#!/usr/bin/env bash
# Bundle Node.js plugins into self-contained single-file scripts.
# Output goes to plugins/<name>/dist/ — ready for distribution without node_modules.
#
# Usage:
#   ./scripts/bundle-plugins.sh          # Bundle all plugins
#   ./scripts/bundle-plugins.sh whatsapp # Bundle specific plugin

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
PLUGINS_DIR="$ROOT/plugins"

info() { printf "\033[36m%s\033[0m\n" "$*"; }
ok()   { printf "\033[32m✓\033[0m %s\n" "$*"; }
err()  { printf "\033[31m✗\033[0m %s\n" "$*" >&2; }

# Require bun
if ! command -v bun >/dev/null 2>&1; then
  err "bun is required for bundling. Install: https://bun.sh"
  exit 1
fi

bundle_whatsapp() {
  local src="$PLUGINS_DIR/whatsapp"
  local dist="$src/dist"
  info "Bundling whatsapp plugin..."

  # Install deps if needed
  if [ ! -d "$src/node_modules" ]; then
    (cd "$src" && bun install --frozen-lockfile 2>/dev/null || bun install)
  fi
  if [ ! -d "$src/whatsapp-connector/node_modules" ]; then
    (cd "$src/whatsapp-connector" && bun install --frozen-lockfile 2>/dev/null || bun install)
  fi

  mkdir -p "$dist/whatsapp-connector"

  bun build "$src/bridge.js" --target=node --outfile="$dist/bridge.js" 2>/dev/null
  bun build "$src/cli.js" --target=node --outfile="$dist/cli.js" 2>/dev/null
  bun build "$src/whatsapp-connector/index.js" --target=node --outfile="$dist/whatsapp-connector/index.js" 2>/dev/null

  # Copy non-JS files needed at runtime
  cp "$src/plugin.toml" "$dist/"
  cp "$src/config.json" "$dist/" 2>/dev/null || true
  cp "$src/prompt.txt" "$dist/" 2>/dev/null || true
  cp "$src"/prompt-dir-*.txt "$dist/" 2>/dev/null || true

  ok "whatsapp → dist/ (bridge.js, cli.js, connector)"
}

bundle_google_workspace() {
  local src="$PLUGINS_DIR/google-workspace"
  local dist="$src/dist"
  info "Bundling google-workspace plugin..."

  mkdir -p "$dist"

  # No external deps — just copy files
  cp "$src/plugin.toml" "$dist/"
  cp "$src/cli.js" "$dist/"
  cp "$src/tools.js" "$dist/"
  cp "$src/tools.json" "$dist/"
  cp "$src/prompt.txt" "$dist/"

  ok "google-workspace → dist/ (no bundling needed, zero deps)"
}

bundle_gmail() {
  local src="$PLUGINS_DIR/gmail"
  local dist="$src/dist"
  info "Bundling gmail plugin..."

  mkdir -p "$dist"

  # No external deps — just copy files
  cp "$src/plugin.toml" "$dist/"
  cp "$src/cli.js" "$dist/"
  cp "$src/tools.js" "$dist/"
  cp "$src/tools.json" "$dist/"
  cp "$src/prompt.txt" "$dist/"

  ok "gmail → dist/ (no bundling needed, zero deps)"
}

bundle_docker_monitor() {
  local src="$PLUGINS_DIR/docker-monitor"
  local dist="$src/dist"
  info "Bundling docker-monitor plugin..."

  mkdir -p "$dist"

  # Bash plugin — just copy files
  cp "$src/plugin.toml" "$dist/"
  cp "$src/monitor.sh" "$dist/"
  chmod +x "$dist/monitor.sh"

  ok "docker-monitor → dist/ (bash script, no deps)"
}

bundle_security_monitor() {
  local src="$PLUGINS_DIR/security-monitor"
  local dist="$src/dist"
  info "Bundling security-monitor plugin..."

  mkdir -p "$dist"

  # Bash plugin — just copy files
  cp "$src/plugin.toml" "$dist/"
  cp "$src/monitor.sh" "$dist/"
  chmod +x "$dist/monitor.sh"

  ok "security-monitor → dist/ (bash script, no deps)"
}

bundle_nano_banana_pro() {
  local src="$PLUGINS_DIR/nano-banana-pro"
  local dist="$src/dist"
  info "Bundling nano-banana-pro plugin..."

  mkdir -p "$dist"

  # Python plugin — just copy files
  cp "$src/plugin.toml" "$dist/"
  cp "$src/tools.json" "$dist/"
  cp "$src/tools.py" "$dist/"

  ok "nano-banana-pro → dist/ (python script, no deps)"
}

# ── Main ──────────────────────────────────────────────────────────────

TARGET="${1:-all}"

case "$TARGET" in
  whatsapp)
    bundle_whatsapp
    ;;
  google-workspace|google)
    bundle_google_workspace
    ;;
  gmail)
    bundle_gmail
    ;;
  docker-monitor|docker)
    bundle_docker_monitor
    ;;
  security-monitor|security)
    bundle_security_monitor
    ;;
  nano-banana-pro|nano-banana)
    bundle_nano_banana_pro
    ;;
  all)
    bundle_whatsapp
    bundle_google_workspace
    bundle_gmail
    bundle_docker_monitor
    bundle_security_monitor
    bundle_nano_banana_pro
    ;;
  *)
    err "Unknown plugin: $TARGET"
    echo "Available: whatsapp, google-workspace, gmail, docker-monitor, security-monitor, nano-banana-pro, all"
    exit 1
    ;;
esac

echo ""
ok "Bundle complete. Distributable files are in plugins/*/dist/"
