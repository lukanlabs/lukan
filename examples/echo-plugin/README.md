# Lukan Plugin Development Guide

This directory contains an example plugin that demonstrates the lukan plugin protocol. Use it as a starting point for building your own plugins.

## Quick Start

```bash
# Install the echo plugin
lukan plugin install ./examples/echo-plugin

# Start it
lukan plugin start echo

# Check status
lukan echo status

# View logs
lukan echo logs -f

# Stop it
lukan echo stop

# Remove it
lukan plugin remove echo
```

## Plugin Structure

A plugin is a directory with at minimum a `plugin.toml` manifest:

```
my-plugin/
├── plugin.toml          # Required: manifest
├── bridge.js            # Your plugin process (any language)
├── cli.js               # Optional: custom command handler
├── prompt.txt           # Optional: extra system prompt injected into the agent
├── tools.json           # Optional: custom tools (for "tools" type plugins)
└── config.json          # Auto-generated at runtime by `lukan <alias> config`
```

## plugin.toml Reference

```toml
[plugin]
name = "my-plugin"              # Required: unique plugin name
version = "0.1.0"               # Required: semver version
description = "What it does"    # Recommended: shown in `lukan plugin list`
plugin_type = "channel"         # "channel" (messaging) or "tools" (adds agent tools)
protocol_version = 1            # Protocol version (currently 1)
alias = "mp"                    # Optional: CLI shortcut → `lukan mp start`, `lukan mp status`

[run]
command = "node"                # Executable to run
args = ["bridge.js"]            # Arguments passed to the command
# env = { KEY = "value" }       # Optional: extra environment variables

# ── Config Schema ──────────────────────────────────────────────
# Declare fields here to enable `lukan <alias> config <key> <action> <value>`

[config.api_key]
type = "string"                 # Types: "string", "string[]", "number", "bool"
description = "API key for the service"

[config.port]
type = "number"
description = "Server port"

[config.languages]
type = "string[]"
description = "Supported languages"
valid_values = ["en", "es", "fr", "de", "pt"]  # Optional: restrict allowed values

[config.enabled]
type = "bool"
description = "Enable/disable the plugin"

# ── Custom Commands ────────────────────────────────────────────
# Declare CLI commands handled by a script (cli.js, cli.sh, etc.)
# Usage: `lukan <alias> <command>`

[commands.auth]
description = "Authenticate with the service"
handler = "auth"                # Passed as first arg to cli.js / cli handler

[commands.status]
description = "Show connection status"
handler = "status"
```

### Config CLI

Once config fields are declared, users can manage them:

```bash
# String fields
lukan mp config api_key set "sk-123"
lukan mp config api_key unset

# String array fields
lukan mp config languages add "en"
lukan mp config languages remove "en"
lukan mp config languages list
lukan mp config languages clear

# Bool fields
lukan mp config enabled on
lukan mp config enabled off

# Number fields
lukan mp config port set 8080

# Show all config
lukan mp config
```

Config is stored at `~/.config/lukan/plugins/<name>/config.json`.

## Plugin Protocol

Plugins communicate with lukan over **stdin/stdout using JSON lines** (one JSON object per line). Stderr is captured to `plugin.log`.

### Lifecycle

```
  lukan (host)                        plugin (process)
  ────────────                        ────────────────
       │                                     │
       │──── spawn process ─────────────────►│
       │                                     │
       │──── Init ──────────────────────────►│
       │     { type, name, config,           │
       │       protocolVersion }             │
       │                                     │
       │◄─── Ready ─────────────────────────│
       │     { type, version,                │
       │       capabilities }                │
       │                                     │
       │◄─── Status ────────────────────────│
       │     { type, status: "connected" }   │
       │                                     │
       │         ┌──── message loop ────┐    │
       │         │                      │    │
       │◄────────│── ChannelMessage ────│───│
       │         │   { type, requestId, │    │
       │         │     sender, content }│    │
       │         │                      │    │
       │─────────│── AgentResponse ────►│───│
       │         │   { type, requestId, │    │
       │         │     text, isError }  │    │
       │         │                      │    │
       │         └──────────────────────┘    │
       │                                     │
       │──── Shutdown ──────────────────────►│
       │                                     │
       │◄─── process exits ─────────────────│
```

### Host → Plugin Messages (stdin)

#### `init`
Sent once after spawning. Contains the plugin name, config values, and protocol version.

```json
{
  "type": "init",
  "name": "my-plugin",
  "config": { "port": 8080, "apiKey": "sk-123" },
  "protocolVersion": 1
}
```

> Note: config keys are camelCase in JSON (snake_case in plugin.toml).

#### `agentResponse`
The agent's reply to a `channelMessage`.

```json
{
  "type": "agentResponse",
  "requestId": "msg-001",
  "text": "The answer is 4.",
  "isError": false
}
```

If the agent encountered an error processing the message, `isError` will be `true` and `text` will contain the error description.

#### `sendMessage`
Send an outbound message to a channel (e.g. pipeline approval notifications). The plugin should forward this to the external service.

```json
{
  "type": "sendMessage",
  "channelId": "chat-123",
  "text": "Pipeline needs your approval..."
}
```

#### `shutdown`
Graceful shutdown request. The plugin should clean up and exit.

```json
{ "type": "shutdown" }
```

### Plugin → Host Messages (stdout)

#### `ready`
Must be sent after receiving `init`. The host waits up to 10 seconds for this.

```json
{
  "type": "ready",
  "version": "0.1.0",
  "capabilities": []
}
```

#### `channelMessage`
An incoming message that should be processed by the agent.

```json
{
  "type": "channelMessage",
  "requestId": "msg-001",
  "sender": "user@example.com",
  "channelId": "chat-123",
  "content": "What is the weather today?"
}
```

- `requestId`: Unique ID to correlate with the `agentResponse`
- `sender`: Display name or identifier of the message author
- `channelId`: Channel/chat/conversation identifier
- `content`: The message text

#### `status`
Report plugin connection status. Shown in `lukan plugin status`.

```json
{ "type": "status", "status": "connected" }
```

Valid statuses: `connected`, `disconnected`, `reconnecting`, `authenticating`.

#### `log`
Write a log entry (appears in `plugin.log` and `lukan <alias> logs`).

```json
{ "type": "log", "level": "info", "message": "Connected to service" }
```

Valid levels: `debug`, `info`, `warn`, `error`.

#### `error`
Report an error to the host.

```json
{ "type": "error", "message": "Connection lost", "recoverable": true }
```

- `recoverable: true` — Host logs the error but keeps the plugin running
- `recoverable: false` — Host stops the plugin

## Writing a Plugin

### Any Language

Plugins can be written in **any language** that can read stdin and write stdout line by line. Here are minimal examples:

#### Bash

See `echo-bridge.sh` in this directory.

#### Python

```python
#!/usr/bin/env python3
import json, sys, uuid

def send(msg):
    print(json.dumps(msg), flush=True)

def log(msg):
    print(f"[my-plugin] {msg}", file=sys.stderr, flush=True)

for line in sys.stdin:
    msg = json.loads(line.strip())

    if msg["type"] == "init":
        log(f"Init received, config: {msg['config']}")
        send({"type": "ready", "version": "0.1.0", "capabilities": []})
        send({"type": "status", "status": "connected"})

    elif msg["type"] == "agentResponse":
        log(f"Agent replied to {msg['requestId']}: {msg['text'][:80]}")
        # Forward the response to your service, send next message, etc.

    elif msg["type"] == "shutdown":
        log("Shutting down")
        sys.exit(0)
```

#### Node.js

```javascript
const readline = require("readline");
const rl = readline.createInterface({ input: process.stdin });

function send(msg) {
  process.stdout.write(JSON.stringify(msg) + "\n");
}

rl.on("line", (line) => {
  const msg = JSON.parse(line);

  if (msg.type === "init") {
    console.error(`[my-plugin] Init, config:`, msg.config);
    send({ type: "ready", version: "0.1.0", capabilities: [] });
    send({ type: "status", status: "connected" });
  } else if (msg.type === "agentResponse") {
    console.error(`[my-plugin] Agent replied: ${msg.text.slice(0, 80)}`);
  } else if (msg.type === "shutdown") {
    process.exit(0);
  }
});
```

#### Rust

For Rust plugins, see `plugins/whisper/` which demonstrates:
- Compiled binary plugin with `[run] command = "lukan-whisper"`
- Reading JSON lines from stdin with `serde_json`
- Config handling, custom commands, and an HTTP server

### Plugin Types

#### `channel` — Messaging Plugins
Channel plugins forward messages between an external service and the lukan agent. The plugin sends `channelMessage` when a user sends a message, and receives `agentResponse` with the agent's reply to forward back.

Examples: WhatsApp, Telegram, Slack, Discord, Email

#### `tools` — Tool Plugins
Tool plugins add new tools to the agent without running a long-lived process. They provide a `tools.json` file and optionally a `prompt.txt` with instructions for the agent.

Example: Google Workspace (adds Sheets, Calendar, Drive tools)

Tools plugin structure:
```
my-tools-plugin/
├── plugin.toml      # plugin_type = "tools", no [run] section needed
├── tools.json       # Tool definitions (JSON schema)
├── tools.js         # Tool execution handler
└── prompt.txt       # Instructions for the agent on how to use the tools
```

## Installation & Distribution

### Local Install (development)
```bash
lukan plugin install ./path/to/my-plugin
```

### Remote Install (from registry)
```bash
# List available plugins
lukan plugin list-remote

# Install by name
lukan plugin install whisper
```

### Plugin Registry
Plugins can be published to the lukan registry by adding an entry to `registry.toml` in the main repo. Binary plugins are distributed as platform-specific tarballs attached to GitHub releases.

## Tips

- **Always flush stdout** after writing a JSON line. Buffered output will hang the protocol.
- **Use stderr for logging.** Stdout is reserved for protocol messages. Everything on stderr goes to `plugin.log`.
- **Keep `requestId` unique.** Use UUIDs or incrementing counters. The host uses this to match responses.
- **Handle `shutdown` gracefully.** Save state, close connections, then exit.
- **Test locally first:** `lukan plugin install ./my-plugin && lukan plugin start my-plugin`
- **Check logs:** `lukan <alias> logs -f` to see real-time stderr + protocol logs.
