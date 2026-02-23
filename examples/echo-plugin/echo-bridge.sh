#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────────────
# Echo Plugin — Example implementation of the lukan plugin protocol.
#
# Protocol: JSON lines over stdin (host→plugin) and stdout (plugin→host).
# Stderr is captured to plugin.log.
#
# Lifecycle:
#   1. Host spawns this process
#   2. Host sends "init" with config
#   3. Plugin replies "ready"
#   4. Plugin sends "channelMessage" → host replies "agentResponse"
#   5. Host sends "shutdown" → plugin exits
#
# See README.md for the full protocol reference.
# ──────────────────────────────────────────────────────────────────────────────

set -euo pipefail

# ── Helpers ──────────────────────────────────────────────────────────────────

# Log to stderr (redirected to plugin.log by the host)
log() { echo "[echo] $*" >&2; }

# Send a JSON message to stdout (host reads this)
# IMPORTANT: always flush stdout — buffered output will hang the protocol.
send() { echo "$1"; }

# Extract a JSON string field (simple, no jq dependency)
json_field() {
  echo "$1" | grep -oP "\"$2\"\s*:\s*\"?\K[^\",}]+" | head -1
}

# ── State ────────────────────────────────────────────────────────────────────

GREETING=""
MSG_COUNT=0

# ── Main loop ────────────────────────────────────────────────────────────────

log "Process started, waiting for Init..."

while IFS= read -r line; do
  type=$(json_field "$line" "type")

  case "$type" in

    # ── Init: host sends config, we reply Ready ────────────────────────────
    init)
      # Extract config values (keys are camelCase in JSON)
      GREETING=$(json_field "$line" "greeting" || echo "")
      log "Received Init (greeting=${GREETING:-<none>})"

      # Reply Ready — host waits up to 10s for this
      send '{"type":"ready","version":"0.1.0","capabilities":[]}'

      # Report connected status (shown in `lukan echo status`)
      send '{"type":"status","status":"connected"}'

      # Send a test message after a short delay
      sleep 1
      MSG_COUNT=$((MSG_COUNT + 1))
      log "Sending test message #${MSG_COUNT}"
      send '{"type":"channelMessage","requestId":"echo-001","sender":"echo-test","channelId":"echo-chan","content":"What is 2+2? Reply with just the number."}'
      ;;

    # ── AgentResponse: the agent replied to our channelMessage ─────────────
    agentResponse)
      request_id=$(json_field "$line" "requestId")
      text=$(json_field "$line" "text")
      is_error=$(json_field "$line" "isError")

      if [ "$is_error" = "true" ]; then
        log "Agent error for ${request_id}: ${text}"
      else
        log "Agent replied to ${request_id}: ${text}"
      fi

      # Log via protocol (also appears in plugin.log)
      send '{"type":"log","level":"info","message":"Agent responded successfully"}'

      # Send another test or finish
      if [ "$request_id" = "echo-001" ]; then
        sleep 1
        MSG_COUNT=$((MSG_COUNT + 1))
        log "Sending test message #${MSG_COUNT}"
        send '{"type":"channelMessage","requestId":"echo-002","sender":"echo-test","channelId":"echo-chan","content":"Say hello in Spanish. One word only."}'
      else
        log "All tests completed!"
        sleep 1
        # Non-recoverable error signals the host to stop the plugin cleanly
        send '{"type":"error","message":"Echo test completed successfully","recoverable":false}'
      fi
      ;;

    # ── Shutdown: clean up and exit ────────────────────────────────────────
    shutdown)
      log "Received Shutdown, cleaning up..."
      # Save state, close connections, etc.
      log "Goodbye!"
      exit 0
      ;;

    # ── Unknown message type ───────────────────────────────────────────────
    *)
      log "Unknown message type: $type"
      send "{\"type\":\"log\",\"level\":\"warn\",\"message\":\"Unknown message type: $type\"}"
      ;;

  esac
done

log "stdin closed, exiting"
