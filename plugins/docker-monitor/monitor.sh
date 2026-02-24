#!/usr/bin/env bash
# Docker Monitor Plugin for Lukan
# Monitors Docker container events and emits SystemEvent messages.
# Protocol: JSON lines over stdin/stdout (lukan plugin protocol v1)
set -euo pipefail

# ── Helpers ──────────────────────────────────────────────────────────
send_json() {
  printf '%s\n' "$1"
}

send_log() {
  local level="$1" message="$2"
  send_json "{\"type\":\"log\",\"level\":\"${level}\",\"message\":\"${message}\"}"
}

send_event() {
  local level="$1" detail="$2"
  send_json "{\"type\":\"systemEvent\",\"source\":\"docker-monitor\",\"level\":\"${level}\",\"detail\":\"${detail}\"}"
}

send_error() {
  local message="$1" recoverable="${2:-true}"
  send_json "{\"type\":\"error\",\"message\":\"${message}\",\"recoverable\":${recoverable}}"
}

# ── Read Init message ────────────────────────────────────────────────
read -r init_line
init_type=$(echo "$init_line" | jq -r '.type // empty' 2>/dev/null || true)

if [[ "$init_type" != "init" ]]; then
  send_error "Expected init message, got: ${init_type:-empty}" false
  exit 1
fi

# Parse container filter from config
containers_json=$(echo "$init_line" | jq -r '.config.containers // empty' 2>/dev/null || true)
CONTAINERS=()
if [[ -n "$containers_json" && "$containers_json" != "null" ]]; then
  while IFS= read -r name; do
    [[ -n "$name" ]] && CONTAINERS+=("$name")
  done < <(echo "$containers_json" | jq -r '.[]' 2>/dev/null)
fi

# ── Send Ready ───────────────────────────────────────────────────────
send_json '{"type":"ready","version":"0.1.0","capabilities":["systemEvent"]}'

# ── Check Docker availability ────────────────────────────────────────
if ! command -v docker &>/dev/null; then
  send_error "docker command not found" false
  exit 1
fi

if ! docker info &>/dev/null; then
  send_error "Cannot connect to Docker daemon. Is Docker running?" false
  exit 1
fi

if [[ ${#CONTAINERS[@]} -gt 0 ]]; then
  send_log "info" "Monitoring containers: ${CONTAINERS[*]}"
else
  send_log "info" "Monitoring all containers"
fi

# ── Handle shutdown ──────────────────────────────────────────────────
cleanup() {
  [[ -n "${STDIN_PID:-}" ]] && kill "$STDIN_PID" 2>/dev/null || true
  exit 0
}
trap cleanup SIGTERM SIGINT SIGHUP

# Read stdin in background to detect shutdown messages
(
  while IFS= read -r line; do
    msg_type=$(echo "$line" | jq -r '.type // empty' 2>/dev/null || true)
    if [[ "$msg_type" == "shutdown" ]]; then
      kill $$ 2>/dev/null || true
      break
    fi
  done
) &
STDIN_PID=$!

# ── Build docker events filter args ─────────────────────────────────
DOCKER_FILTER_ARGS=(
  --filter 'type=container'
  --filter 'event=die'
  --filter 'event=oom'
  --filter 'event=health_status'
  --filter 'event=start'
  --filter 'event=stop'
  --filter 'event=kill'
)

for name in "${CONTAINERS[@]+"${CONTAINERS[@]}"}"; do
  DOCKER_FILTER_ARGS+=(--filter "container=${name}")
done

# ── Main event loop ─────────────────────────────────────────────────
# Use process substitution (not pipe) so the while loop runs in the
# main shell. This avoids subshell pipe buffering — printf goes
# directly to the real stdout which the host reads line-by-line.
send_log "info" "Starting Docker event monitor..."

while IFS= read -r event_json; do
  # Parse event fields
  action=$(echo "$event_json" | jq -r '.Action // .status // empty' 2>/dev/null || true)
  container_name=$(echo "$event_json" | jq -r '.Actor.Attributes.name // empty' 2>/dev/null || true)
  container_id=$(echo "$event_json" | jq -r '.id // empty' 2>/dev/null | head -c 12 || true)
  image=$(echo "$event_json" | jq -r '.Actor.Attributes.image // empty' 2>/dev/null || true)

  label="${container_name:-${container_id}}"
  [[ -z "$label" ]] && continue

  case "$action" in
    die)
      exit_code=$(echo "$event_json" | jq -r '.Actor.Attributes.exitCode // "unknown"' 2>/dev/null || true)
      if [[ "$exit_code" == "0" ]]; then
        send_event "info" "Container '${label}' exited cleanly (code 0)"
      elif [[ "$exit_code" == "137" ]]; then
        send_event "warn" "Container '${label}' was killed (SIGKILL, code 137) — image: ${image}"
      elif [[ "$exit_code" == "143" ]]; then
        send_event "info" "Container '${label}' terminated gracefully (SIGTERM, code 143)"
      else
        send_event "error" "Container '${label}' crashed (exit code ${exit_code}) — image: ${image}"
      fi
      ;;
    oom)
      send_event "critical" "Container '${label}' killed by OOM (out of memory) — image: ${image}"
      ;;
    health_status:healthy)
      send_event "info" "Container '${label}' is now healthy"
      ;;
    health_status:unhealthy)
      send_event "warn" "Container '${label}' is now UNHEALTHY — image: ${image}"
      ;;
    start)
      send_event "info" "Container '${label}' started — image: ${image}"
      ;;
    stop)
      send_event "info" "Container '${label}' stopped"
      ;;
    kill)
      signal=$(echo "$event_json" | jq -r '.Actor.Attributes.signal // "unknown"' 2>/dev/null || true)
      send_event "warn" "Container '${label}' received signal ${signal}"
      ;;
    *)
      # Ignore unknown actions
      ;;
  esac
done < <(docker events --format '{{json .}}' "${DOCKER_FILTER_ARGS[@]}" 2>/dev/null)

send_log "warn" "Docker event stream ended"
kill "$STDIN_PID" 2>/dev/null || true
