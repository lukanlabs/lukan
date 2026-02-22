#!/usr/bin/env bash
# Echo plugin — implements the lukan plugin protocol over stdin/stdout (JSON lines).
# Reads Init, sends Ready, then sends a test ChannelMessage and waits for AgentResponse.

set -euo pipefail

# Log to stderr (redirected to plugin.log by the host)
log() { echo "[echo-plugin] $*" >&2; }

# Extract a JSON field value (simple grep-based, no python needed)
json_field() {
  echo "$1" | grep -oP "\"$2\"\s*:\s*\"?\K[^\",}]+" | head -1
}

log "Process started, waiting for Init..."

# Read lines from stdin
while IFS= read -r line; do
  type=$(json_field "$line" "type")

  case "$type" in
    init)
      log "Received Init, sending Ready"
      echo '{"type":"ready","version":"0.1.0","capabilities":[]}'

      # Send status connected
      echo '{"type":"status","status":"connected"}'

      # Send a test message after a short delay
      sleep 1
      log "Sending test channelMessage"
      echo '{"type":"channelMessage","requestId":"test-001","sender":"test-user","channelId":"echo-test-001","content":"What is 2+2? Reply with just the number."}'
      ;;

    agentResponse)
      request_id=$(json_field "$line" "requestId")
      log "Got AgentResponse for request: $request_id"

      # Log success
      echo '{"type":"log","level":"info","message":"Agent responded successfully!"}'

      # Send one more test, then we are done
      if [ "$request_id" = "test-001" ]; then
        sleep 1
        echo '{"type":"channelMessage","requestId":"test-002","sender":"test-user","channelId":"echo-test-002","content":"Say hello in Spanish. One word only."}'
      else
        log "All tests passed! Shutting down."
        sleep 1
        # Send a non-recoverable error to signal we are done (clean exit)
        echo '{"type":"error","message":"All tests completed successfully","recoverable":false}'
      fi
      ;;

    shutdown)
      log "Received Shutdown, exiting"
      exit 0
      ;;

    *)
      log "Unknown message type: $type — line: $line"
      ;;
  esac
done

log "stdin closed, exiting"
