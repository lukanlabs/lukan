# lukan Makefile

VERSION ?= $(shell git describe --tags --match 'v*' --always 2>/dev/null || echo "dev")
COMMIT ?= $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")

BINARY_NAME := lukan
R2_BUCKET ?= lukan-releases
GH_REPO ?= lukanlabs/lukan

# Detect current platform
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
ifeq ($(UNAME_S),Linux)
  OS := linux
else ifeq ($(UNAME_S),Darwin)
  OS := darwin
else
  OS := $(UNAME_S)
endif
ifeq ($(UNAME_M),x86_64)
  ARCH := amd64
else ifeq ($(UNAME_M),aarch64)
  ARCH := arm64
else ifeq ($(UNAME_M),arm64)
  ARCH := arm64
else
  ARCH := $(UNAME_M)
endif
PLATFORM := $(OS)-$(ARCH)

# Cross-platform sha256: macOS has shasum, Linux has sha256sum
ifeq ($(UNAME_S),Darwin)
  SHA256CMD := shasum -a 256
else
  SHA256CMD := sha256sum
endif

.PHONY: all build clean test check install release bundle-plugins package-plugins package-whisper bundle-desktop upload upload-daily upload-gh help

all: build

## build: Build release binary for current platform (includes desktop frontend)
build:
	@if [ -d desktop-client ]; then \
		echo "Building desktop frontend..."; \
		(cd desktop-client && bun install --frozen-lockfile 2>/dev/null || bun install && bun run build); \
		echo "Forcing desktop crate rebuild (frontend changed)..."; \
		touch crates/lukan-desktop/build.rs; \
		rm -f target/release/lukan-desktop target/release/deps/lukan_desktop-* 2>/dev/null || true; \
		rm -rf target/release/build/lukan-desktop-* 2>/dev/null || true; \
	fi
	cargo build --release -p lukan -p lukan-relay
	cargo build --release -p lukan-desktop

## build-debug: Build debug binary
build-debug:
	cargo build

## clean: Clean build artifacts
clean:
	cargo clean
	rm -rf dist plugins/*/dist

## test: Run tests
test:
	cargo test --workspace

## check: Run fmt, clippy, and tests
check:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings
	cargo test --workspace

## install: Build and install to ~/.local/bin
install: build
	mkdir -p $(HOME)/.local/bin
	cp target/release/$(BINARY_NAME) $(HOME)/.local/bin/

## bundle-plugins: Bundle Node.js plugins into self-contained scripts
bundle-plugins:
	./scripts/bundle-plugins.sh

## package-plugins: Create distributable plugin tarballs in dist/plugins/
package-plugins: bundle-plugins
	@mkdir -p dist/plugins
	@# WhatsApp plugin
	@cd plugins/whatsapp/dist && tar czf ../../../dist/plugins/lukan-plugin-whatsapp.tar.gz .
	@echo "  Packaged: lukan-plugin-whatsapp.tar.gz"
	@# Email plugin
	@cd plugins/email/dist && tar czf ../../../dist/plugins/lukan-plugin-email.tar.gz .
	@echo "  Packaged: lukan-plugin-email.tar.gz"
	@# Google Workspace plugin
	@cd plugins/google-workspace/dist && tar czf ../../../dist/plugins/lukan-plugin-google-workspace.tar.gz .
	@echo "  Packaged: lukan-plugin-google-workspace.tar.gz"
	@# Gmail plugin
	@cd plugins/gmail/dist && tar czf ../../../dist/plugins/lukan-plugin-gmail.tar.gz .
	@echo "  Packaged: lukan-plugin-gmail.tar.gz"
	@# Docker Monitor plugin
	@cd plugins/docker-monitor/dist && tar czf ../../../dist/plugins/lukan-plugin-docker-monitor.tar.gz .
	@echo "  Packaged: lukan-plugin-docker-monitor.tar.gz"
	@# Security Monitor plugin
	@cd plugins/security-monitor/dist && tar czf ../../../dist/plugins/lukan-plugin-security-monitor.tar.gz .
	@echo "  Packaged: lukan-plugin-security-monitor.tar.gz"
	@# Nano Banana Pro plugin
	@cd plugins/nano-banana-pro/dist && tar czf ../../../dist/plugins/lukan-plugin-nano-banana-pro.tar.gz .
	@echo "  Packaged: lukan-plugin-nano-banana-pro.tar.gz"
	@# Telegram plugin
	@cd plugins/telegram/dist && tar czf ../../../dist/plugins/lukan-plugin-telegram.tar.gz .
	@echo "  Packaged: lukan-plugin-telegram.tar.gz"
	@# Slack plugin
	@cd plugins/slack/dist && tar czf ../../../dist/plugins/lukan-plugin-slack.tar.gz .
	@echo "  Packaged: lukan-plugin-slack.tar.gz"
	@# Discord plugin
	@cd plugins/discord/dist && tar czf ../../../dist/plugins/lukan-plugin-discord.tar.gz .
	@echo "  Packaged: lukan-plugin-discord.tar.gz"

## package-whisper: Build whisper plugin binary and create platform-specific tarball
WHISPER_ARCH := $(if $(filter x86_64 x86,$(UNAME_M)),x86_64,$(if $(filter aarch64 arm64,$(UNAME_M)),aarch64,$(UNAME_M)))
WHISPER_OS := $(if $(filter Linux,$(UNAME_S)),linux,$(if $(filter Darwin,$(UNAME_S)),macos,$(UNAME_S)))
WHISPER_PLATFORM := $(WHISPER_OS)-$(WHISPER_ARCH)
package-whisper:
	@echo "Building whisper plugin ($(WHISPER_PLATFORM))..."
	cd plugins/whisper && cargo build --release
	@mkdir -p dist/plugins plugins/whisper/dist
	@cp plugins/whisper/target/release/lukan-whisper plugins/whisper/dist/
	@cp plugins/whisper/plugin.toml plugins/whisper/dist/
	@cd plugins/whisper/dist && tar czf ../../../dist/plugins/lukan-plugin-whisper-$(WHISPER_PLATFORM).tar.gz .
	@echo "  Packaged: lukan-plugin-whisper-$(WHISPER_PLATFORM).tar.gz"

## bundle-desktop: Build Tauri desktop bundles (.deb, .AppImage, .dmg)
bundle-desktop: build
	@echo "Building Tauri desktop bundles..."
	@cp target/release/$(BINARY_NAME) target/release/$(BINARY_NAME) 2>/dev/null || true
	cd crates/lukan-desktop && cargo tauri build
	@mkdir -p dist/desktop
	@for f in target/release/bundle/deb/*.deb; do [ -f "$$f" ] && cp "$$f" dist/desktop/"$$(basename "$$f" | tr ' ' '_')"; done
	@for f in target/release/bundle/rpm/*.rpm; do [ -f "$$f" ] && cp "$$f" dist/desktop/"$$(basename "$$f" | tr ' ' '_')"; done
	@for f in target/release/bundle/appimage/*.AppImage; do [ -f "$$f" ] && cp "$$f" dist/desktop/"$$(basename "$$f" | tr ' ' '_')"; done
	@for f in target/release/bundle/dmg/*.dmg; do [ -f "$$f" ] && cp "$$f" dist/desktop/"$$(basename "$$f" | tr ' ' '_')"; done
	@for f in target/release/bundle/macos/*.app.tar.gz; do [ -f "$$f" ] && cp "$$f" dist/desktop/"$$(basename "$$f" | tr ' ' '_')"; done
	@echo "Desktop bundles in dist/desktop/"
	@ls -lh dist/desktop/ 2>/dev/null || true

## release: Build binary + bundle plugins + generate checksums
release: build bundle-plugins package-plugins
	@$(MAKE) package-whisper || echo "  Warning: whisper plugin build failed (skipped)"
	@echo "Building release $(VERSION) for $(PLATFORM)..."
	@mkdir -p dist
	@cp target/release/$(BINARY_NAME) dist/$(BINARY_NAME)-$(PLATFORM)
	@if [ -f target/release/$(BINARY_NAME)-desktop ]; then \
		cp target/release/$(BINARY_NAME)-desktop dist/$(BINARY_NAME)-desktop-$(PLATFORM); \
	fi
	@if [ -f target/release/$(BINARY_NAME)-relay ]; then \
		cp target/release/$(BINARY_NAME)-relay dist/$(BINARY_NAME)-relay-$(PLATFORM); \
	fi
	@# Generate checksums
	@cd dist && $(SHA256CMD) $(BINARY_NAME)-$(PLATFORM) > checksums.txt
	@if [ -f dist/$(BINARY_NAME)-desktop-$(PLATFORM) ]; then \
		cd dist && $(SHA256CMD) $(BINARY_NAME)-desktop-$(PLATFORM) >> checksums.txt; \
	fi
	@if [ -f dist/$(BINARY_NAME)-relay-$(PLATFORM) ]; then \
		cd dist && $(SHA256CMD) $(BINARY_NAME)-relay-$(PLATFORM) >> checksums.txt; \
	fi
	@cd dist && $(SHA256CMD) ../install.sh | sed 's#  \.\./install\.sh$$#  install.sh#' >> checksums.txt
	@cd dist/plugins && $(SHA256CMD) *.tar.gz >> ../checksums.txt
	@# Write version file
	@echo "$(VERSION)" > dist/latest
	@echo ""
	@echo "Release $(VERSION) built in dist/"
	@echo "  Binary:  dist/$(BINARY_NAME)-$(PLATFORM)"
	@echo "  Desktop: dist/$(BINARY_NAME)-desktop-$(PLATFORM)"
	@echo "  Relay:   dist/$(BINARY_NAME)-relay-$(PLATFORM)"
	@echo "  Plugins: dist/plugins/*.tar.gz"
	@echo "  Checksums: dist/checksums.txt"

## upload: Upload release to Cloudflare R2
upload: release
	@echo "Uploading $(VERSION) to R2:$(R2_BUCKET)..."
	bunx wrangler r2 object put --remote $(R2_BUCKET)/$(BINARY_NAME)-$(PLATFORM) --file dist/$(BINARY_NAME)-$(PLATFORM)
	@if [ -f dist/$(BINARY_NAME)-desktop-$(PLATFORM) ]; then \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/$(BINARY_NAME)-desktop-$(PLATFORM) --file dist/$(BINARY_NAME)-desktop-$(PLATFORM); \
	fi
	@if [ -f dist/$(BINARY_NAME)-relay-$(PLATFORM) ]; then \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/$(BINARY_NAME)-relay-$(PLATFORM) --file dist/$(BINARY_NAME)-relay-$(PLATFORM); \
	fi
	bunx wrangler r2 object put --remote $(R2_BUCKET)/checksums.txt --file dist/checksums.txt --cache-control "public, max-age=60"
	bunx wrangler r2 object put --remote $(R2_BUCKET)/latest --file dist/latest --cache-control "public, max-age=60"
	bunx wrangler r2 object put --remote $(R2_BUCKET)/install.sh --file install.sh --cache-control "public, max-age=60"
	@# Upload plugins
	@for f in dist/plugins/*.tar.gz; do \
		name=$$(basename $$f); \
		echo "Uploading plugin: $$name"; \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/plugins/$$name --file "$$f" --cache-control "public, max-age=60"; \
	done
	@# Upload desktop bundles
	@if [ -d dist/desktop ]; then \
		for f in dist/desktop/*; do \
			name=$$(basename "$$f"); \
			echo "Uploading desktop bundle: $$name"; \
			bunx wrangler r2 object put --remote "$(R2_BUCKET)/desktop/$$name" --file "$$f" --cache-control "public, max-age=60"; \
		done; \
	fi
	@# Upload registry
	bunx wrangler r2 object put --remote $(R2_BUCKET)/registry.toml --file registry.toml
	@echo ""
	@echo "Upload complete!"
	@echo "Install: curl -fsSL https://get.lukan.ai/install.sh | bash"

## upload-daily: Upload release to R2 daily (unstable) channel
upload-daily: release
	@echo "Uploading $(VERSION) to R2:$(R2_BUCKET)/daily/..."
	bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/$(BINARY_NAME)-$(PLATFORM) --file dist/$(BINARY_NAME)-$(PLATFORM)
	@if [ -f dist/$(BINARY_NAME)-desktop-$(PLATFORM) ]; then \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/$(BINARY_NAME)-desktop-$(PLATFORM) --file dist/$(BINARY_NAME)-desktop-$(PLATFORM); \
	fi
	@if [ -f dist/$(BINARY_NAME)-relay-$(PLATFORM) ]; then \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/$(BINARY_NAME)-relay-$(PLATFORM) --file dist/$(BINARY_NAME)-relay-$(PLATFORM); \
	fi
	bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/checksums.txt --file dist/checksums.txt --cache-control "public, max-age=60"
	bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/latest --file dist/latest --cache-control "public, max-age=60"
	@# Upload plugins
	@for f in dist/plugins/*.tar.gz; do \
		name=$$(basename $$f); \
		echo "Uploading plugin: $$name"; \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/plugins/$$name --file "$$f" --cache-control "public, max-age=60"; \
	done
	@# Upload registry
	bunx wrangler r2 object put --remote $(R2_BUCKET)/daily/registry.toml --file registry.toml
	@echo ""
	@echo "Daily upload complete!"
	@echo "Install: lukan update --daily"

## upload-gh: Upload release to GitHub Releases
upload-gh: release
	@echo "Uploading $(VERSION) to GitHub Releases..."
	@DESKTOP_FILE=""; \
	if [ -f dist/$(BINARY_NAME)-desktop-$(PLATFORM) ]; then \
		DESKTOP_FILE="dist/$(BINARY_NAME)-desktop-$(PLATFORM)"; \
	fi; \
	if gh release view $(VERSION) --repo $(GH_REPO) >/dev/null 2>&1; then \
		echo "Release $(VERSION) exists, overwriting assets..."; \
		gh release upload $(VERSION) --repo $(GH_REPO) --clobber \
			dist/$(BINARY_NAME)-$(PLATFORM) $$DESKTOP_FILE dist/checksums.txt dist/latest install.sh \
			dist/plugins/*.tar.gz; \
	else \
		echo "Creating release $(VERSION)..."; \
		gh release create $(VERSION) --repo $(GH_REPO) \
			--title "$(VERSION)" \
			--notes "Release $(VERSION)" \
			dist/$(BINARY_NAME)-$(PLATFORM) $$DESKTOP_FILE dist/checksums.txt dist/latest install.sh \
			dist/plugins/*.tar.gz; \
	fi
	@echo ""
	@echo "Upload complete!"
	@echo "https://github.com/$(GH_REPO)/releases/tag/$(VERSION)"

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^## //p' $(MAKEFILE_LIST) | column -t -s ':' | sed 's/^/  /'
