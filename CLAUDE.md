# CLAUDE.md — lukan (Rust)

## Overview

Rust rewrite of the lukan AI agent CLI. Cargo workspace with 6 crates.

## Commands

```bash
cargo build                              # Build all crates
cargo run -- chat                        # Start interactive chat
cargo run -- chat --provider anthropic   # Use specific provider
cargo fmt && cargo clippy -- -D warnings && cargo test  # Quality check
```

## Crate Structure

```
lukan-core       # Shared types, config, errors
lukan-providers  # LLM provider implementations
lukan-tools      # Tool system (Bash, ReadFile, etc.)
lukan-search     # Symbol index (tree-sitter)
lukan-agent      # Agent loop, sessions, memory
lukan-tui        # Terminal UI (ratatui)
```

## Conventions

- **Edition**: Rust 2024
- **Errors**: `anyhow::Result<T>` + `.context()`, `thiserror` for custom errors
- **Async**: `tokio` + `#[async_trait]`, bounds `Send + Sync`
- **HTTP**: `reqwest` with `rustls-tls`
- **Serde**: `#[serde(rename_all = "camelCase")]` for JSON compat with TS version
- **Streams**: `Pin<Box<dyn Stream<Item = Result<T>> + Send>>`
- **Config**: XDG paths at `~/.config/lukan/`, same JSON format as TS version
- **Quality**: `cargo fmt && cargo clippy -- -D warnings && cargo test`
- `#![allow(dead_code)]` during active development
