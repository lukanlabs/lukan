# Architecture

Lukan is a Cargo workspace with 11 crates and a shared React frontend. This document explains how the pieces fit together.

## High-Level Overview

```
                    User
                     |
          +----------+----------+
          |          |          |
        [TUI]     [Web]    [Desktop]
          |          |          |
          +----------+----------+
                     |
              [Agent Loop]
              /     |     \
         [Tools] [Providers] [Plugins]
            |        |          |
        [Browser] [LLM APIs] [Channels]
```

All three interfaces (TUI, Web, Desktop) share the same agent loop, tools, and providers. The Web and Desktop UIs share the same React frontend — only the transport layer differs.

## Crate Dependency Graph

```
lukan (binary)
  +-- lukan-tui            Terminal UI
  +-- lukan-web            Web server + WebSocket
  |     +-- lukan-agent    Agent loop
  |     |     +-- lukan-providers   LLM integrations
  |     |     +-- lukan-tools       Tool execution
  |     |     |     +-- lukan-browser   CDP client
  |     |     |     +-- lukan-core      Shared types
  |     |     +-- lukan-core
  |     +-- lukan-plugins  Plugin system
  |     +-- lukan-browser
  +-- lukan-agent
  +-- lukan-plugins

lukan-desktop (separate binary)
  +-- lukan-agent
  +-- lukan-plugins
  +-- lukan-browser
  +-- lukan-core

lukan-relay (separate binary)
  +-- lukan-core
```

## Crate Details

### lukan-core

Foundation crate with zero heavyweight dependencies on other lukan crates.

- **Config** (`config/`): `AppConfig`, `Credentials`, `PermissionRules`, XDG path management
- **Models** (`models/`): `Message`, `ContentBlock`, `ToolDefinition`, `StreamEvent`, `Session`
- **Crypto** (`crypto.rs`): X25519 key exchange, HKDF key derivation, AES-GCM encryption for E2E relay
- **Workers** (`workers.rs`): `WorkerDefinition`, `WorkerManager`, cron scheduling
- **Relay** (`relay.rs`): Relay config types, `RelayToDaemon`/`DaemonToRelay` protocol messages

### lukan-providers

Implements the `Provider` trait for 8 LLM backends.

```rust
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn supports_images(&self) -> bool;
    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()>;
}
```

Each provider handles SSE parsing, authentication, and model-specific quirks (prompt caching, reasoning tokens, schema adaptation).

**Providers**: Anthropic, OpenAI Codex, GitHub Copilot, Fireworks, Nebius, Ollama Cloud, Zai, OpenAI-compatible.

### lukan-tools

Implements the `Tool` trait for agent capabilities.

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult>;
}
```

**Core tools**: Bash, ReadFiles, WriteFile, EditFile, Glob, Grep, WebFetch, LoadSkill, TaskAdd/List/Update.

**Browser tools**: Screenshot, Navigate, Click, Type, EvaluateJS, Snapshot, SavePDF, TabManagement.

**Plugin tools**: Dynamically registered from installed plugins via `tools.json`.

**Security**: `ToolContext` carries sandbox config, allowed paths, sensitive file patterns, and permission mode. `SandboxRunner` wraps Bash execution in bwrap when enabled.

### lukan-agent

The brain. Orchestrates LLM calls, tool execution, and state management.

- **AgentLoop** (`agent_loop.rs`): Main loop — builds prompt, calls provider, parses tool calls, executes tools, repeats. Emits `StreamEvent`s for UI consumption.
- **Permission System**: Three modes (Planner, Auto, Manual) with pattern-based allow/deny rules.
- **Context Management**: Automatic compaction at 150K tokens, memory updates at 50K tokens.
- **Session Management** (`sessions.rs`): Load/save chat history as JSON, session listing and metadata.
- **Sub-agents** (`sub_agent.rs`): Spawn child agents for parallel research tasks.

### lukan-tui

Ratatui-based terminal application (~6K lines).

- Markdown rendering with syntax highlighting (pulldown-cmark + syntect)
- Modal dialogs for tool approval, plan review, and planner questions
- Integrated terminal emulator panel
- Multi-tab support
- Real-time streaming display

### lukan-web

Axum web server that serves the React frontend and handles WebSocket communication.

- **WebSocket Handler** (`ws_handler.rs`): Bridges the React frontend to the agent loop. Handles message routing, tool approval, session management, terminal I/O, and streaming events.
- **REST API**: Endpoints for config, credentials, providers, plugins, workers, files, browser, events, and processes.
- **Static Files**: React SPA embedded via `rust-embed`.
- **Auth**: Optional password + JWT token authentication.

### lukan-desktop

Tauri 2 wrapper that provides native desktop integration.

- PTY management for integrated terminal
- Audio recording (cpal)
- Native file dialogs and menus
- System tray integration
- Relay transport for remote connections

### lukan-browser

Chrome DevTools Protocol client for browser automation.

- Browser lifecycle management (auto-launch Chrome/Chromium/Edge)
- Profile modes: temporary, persistent, custom path
- DOM snapshots, JavaScript evaluation, screenshot capture
- URL guard (blocks navigation to private/internal IPs)
- Tab management

### lukan-plugins

Plugin discovery, lifecycle, and communication.

- **Manager** (`manager.rs`): Scans `~/.config/lukan/plugins/` for `plugin.toml` manifests, spawns plugin processes
- **Channel** (`channel.rs`): IPC bridge between plugins and agent loop
- **Protocol**: JSON lines over stdin/stdout (`Init`, `Ready`, `ChannelMessage`, `AgentResponse`, `Status`, `ViewUpdate`)
- **Registry** (`registry.rs`): Remote plugin registry at `get.lukan.ai`
- **Types**: `channel` (messaging integration) and `tools` (new agent capabilities)

### lukan-search

Symbol indexing using tree-sitter for code navigation. Currently a stub — planned for a future phase.

### lukan-relay

Standalone binary. Acts as a WebSocket proxy between remote browser clients and local daemon instances.

- **Two WebSocket endpoints**: `/ws/client` (browser) and `/ws/daemon` (local agent)
- **Authentication**: Google OAuth + JWT tokens + device code flow for headless auth
- **E2E Encryption**: Messages optionally encrypted end-to-end (X25519 + AES-GCM) so the relay never sees plaintext
- **REST Tunnel**: Proxies HTTP requests from browser through to daemon

## Frontend (desktop-client/)

A single React + TypeScript SPA used by all three UI modes:

```
desktop-client/src/
  lib/
    transport.ts          Transport abstraction interface
    transport-tauri.ts    Desktop mode: Tauri IPC commands
    transport-web.ts      Web mode: WebSocket to local lukan-web
    transport-relay.ts    Relay mode: WebSocket to remote relay server
  components/
    workspace/            Toolbar, ActivityBar, SidePanel, MainArea
    chat/                 Chat UI, message rendering, input
  hooks/
    useWorkspace.ts       Workspace state management
    useBrowser.ts         Browser panel state
    useTerminal.ts        Terminal panel state
```

The transport layer abstracts the connection so the UI code is identical regardless of mode. Detection is automatic: Tauri context uses IPC, relay hostnames use relay WebSocket, everything else uses local WebSocket.

## Data Flow

### Chat Message Flow

```
User Input → [UI] → WebSocket/IPC → [ws_handler] → [AgentLoop]
                                                        |
                                                   [Provider.stream()]
                                                        |
                                                   StreamEvent::TextDelta
                                                        |
                                                   [ws_handler] → WebSocket → [UI] → Display
```

### Tool Execution Flow

```
LLM returns tool_use → [AgentLoop] checks permissions
                            |
                    +-------+-------+
                    |               |
                [Approved]    [Needs approval]
                    |               |
              [Tool.execute()]  StreamEvent::ToolApproval → [UI] → User decides
                    |
              ToolResult → append to messages → next LLM call
```

### Plugin Message Flow

```
WhatsApp message → [Plugin process] → stdin JSON → [PluginManager]
                                                        |
                                                   [AgentLoop] processes
                                                        |
                                                   AgentResponse → stdout JSON → [Plugin] → WhatsApp reply
```

## Key Design Decisions

1. **Trait-based extensibility**: `Provider` and `Tool` traits make it straightforward to add new LLM backends and agent capabilities.

2. **Streaming architecture**: Everything flows through `StreamEvent` — the UI never polls, it reacts to events. This makes all three UIs consistent.

3. **Transport abstraction**: One React SPA, three connection modes. The transport layer is the only code that differs between Desktop, Web, and Relay.

4. **Plugin isolation**: Plugins run as separate processes communicating via JSON over stdio. A crashing or malicious plugin cannot affect the host process.

5. **Security layers**: Permissions (tool-level) + Sandbox (OS-level) + Sensitive patterns (file-level) provide defense in depth.
