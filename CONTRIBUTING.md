# Contributing to Lukan

Thank you for your interest in contributing to Lukan. This guide covers the process for contributing to this project.

## Getting Started

### Prerequisites

- Rust (edition 2024, stable toolchain)
- Bun (for desktop-client frontend)
- Linux or macOS (Windows via WSL)

#### Linux system dependencies

```bash
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf libssl-dev libasound2-dev
```

### Build

#### CLI only (no system dependencies required beyond Rust)

```bash
git clone https://github.com/lukanlabs/lukan.git
cd lukan
cargo build -p lukan
```

#### Full build (CLI + Desktop app)

```bash
cd desktop-client && bun install && bun run build && cd ..
cargo build
```

### Run Quality Checks

Every PR must pass these checks:

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## Project Structure

```
src/                   CLI entry point, subcommands
crates/
  lukan-core/          Shared types, config, errors, crypto
  lukan-providers/     LLM provider implementations
  lukan-tools/         Tool system (Bash, files, search, browser)
  lukan-agent/         Agent loop, sessions, memory, permissions
  lukan-tui/           Terminal UI (ratatui)
  lukan-web/           Web server (Axum) + WebSocket handler
  lukan-desktop/       Tauri desktop wrapper
  lukan-browser/       Chrome DevTools Protocol client
  lukan-plugins/       Plugin framework and IPC
  lukan-search/        Symbol indexing (tree-sitter)
  lukan-relay/         Relay server for remote access
desktop-client/        React + TypeScript frontend
plugins/               Plugin source code
prompts/               System prompts
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed architecture documentation.

## How to Contribute

### Reporting Bugs

Open an issue with:
- Steps to reproduce
- Expected vs actual behavior
- OS, Rust version, and lukan version (`lukan --version`)

### Suggesting Features

Open an issue describing the feature, the use case, and how it fits into the existing architecture.

### Pull Requests

1. Fork the repository
2. Create a branch from `development` (`git checkout -b feature/my-feature development`)
3. Make your changes
4. Run `cargo fmt && cargo clippy -- -D warnings && cargo test`
5. Commit with a clear message (see commit conventions below)
6. Open a PR against `development`

### Commit Conventions

We use conventional commits:

```
feat(agent): add support for streaming tool results
fix(tui): resolve markdown rendering for nested lists
docs: update provider setup instructions
style: apply cargo fmt
refactor(tools): simplify glob pattern matching
test(core): add config serialization tests
```

Format: `type(scope): description`

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Scopes: `core`, `providers`, `tools`, `agent`, `tui`, `web`, `desktop`, `browser`, `plugins`, `search`, `relay`

## Code Conventions

- **Errors**: `anyhow::Result<T>` with `.context()`, `thiserror` for custom errors
- **Async**: `tokio` + `#[async_trait]`, bounds `Send + Sync`
- **HTTP**: `reqwest` with `rustls-tls`
- **Serde**: `#[serde(rename_all = "camelCase")]` for JSON
- **Streams**: `Pin<Box<dyn Stream<Item = Result<T>> + Send>>`
- **Config**: XDG paths at `~/.config/lukan/`

## Areas for Contribution

### Good First Issues

Look for issues labeled `good-first-issue`. Common areas:

- Adding new LLM providers (implement the `Provider` trait)
- Creating new plugins (WhatsApp, Telegram, Slack, etc.)
- Writing skills for common workflows
- Improving documentation
- Adding tests

### Provider Contributions

To add a new LLM provider:

1. Create a new file in `crates/lukan-providers/src/`
2. Implement the `Provider` trait (see existing providers for reference)
3. Register it in `crates/lukan-providers/src/lib.rs`
4. Add it to the CLI setup wizard in `src/setup.rs`

### Plugin Contributions

To create a new plugin:

1. Create a directory in `plugins/<name>/`
2. Add a `plugin.toml` manifest
3. Implement the plugin binary (any language)
4. Follow the IPC protocol (JSON lines over stdin/stdout)

See `plugins/email/` for a reference implementation.

#### Building and installing plugins locally

Plugins can be installed from a local path without downloading from the registry:

```bash
# Install directly from source (runs bun/npm install automatically)
lukan plugin install ./plugins/whatsapp

# Or bundle first, then install the optimized version
./scripts/bundle-plugins.sh whatsapp
lukan plugin install ./plugins/whatsapp
```

When installing from a local path, lukan will prefer `dist/` (bundled) if it exists, otherwise it installs from source directly. Dependencies are resolved automatically via `bun install` or `npm install`.

To bundle all plugins at once:

```bash
./scripts/bundle-plugins.sh
```

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
