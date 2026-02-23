# lukan Makefile (Rust)

VERSION ?= $(shell v=$$(git describe --tags --match 'v*' --always 2>/dev/null || echo "dev"); if [ -n "$$(git status --porcelain -- . 2>/dev/null)" ]; then v="$$v-dirty"; fi; echo "$$v")
COMMIT ?= $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
BUILD_DATE ?= $(shell date -u +"%Y-%m-%dT%H:%M:%SZ")

# Binary name
BINARY_NAME := lukan

# Cloudflare R2 bucket for releases
R2_BUCKET ?= lukan-releases

# Rust cross-compilation targets
TARGET_LINUX_AMD64  := x86_64-unknown-linux-gnu
TARGET_LINUX_ARM64  := aarch64-unknown-linux-gnu
TARGET_DARWIN_AMD64 := x86_64-apple-darwin
TARGET_DARWIN_ARM64 := aarch64-apple-darwin

.PHONY: all build clean test release checksums upload upload-gh install install-local bundle-plugins help

all: build

## build: Build release binary for current platform
build:
	cargo build --release

## build-debug: Build debug binary
build-debug:
	cargo build

## clean: Clean build artifacts
clean:
	cargo clean
	rm -rf dist

## test: Run tests
test:
	cargo test

## check: Run fmt, clippy, and tests
check:
	cargo fmt --check
	cargo clippy -- -D warnings
	cargo test

## install: Install to ~/.local/bin
install: build
	mkdir -p $(HOME)/.local/bin
	cp target/release/$(BINARY_NAME) $(HOME)/.local/bin/

## install-local: Install to /usr/local/bin (requires sudo)
install-local: build
	sudo cp target/release/$(BINARY_NAME) /usr/local/bin/

## bundle-plugins: Bundle Node.js plugins into self-contained scripts
bundle-plugins:
	./scripts/bundle-plugins.sh

## release: Build release binaries for all platforms (uses cross)
release: bundle-plugins
	@echo "Building release binaries ($(VERSION))..."
	@mkdir -p dist
	cross build --release --target $(TARGET_LINUX_AMD64)
	cross build --release --target $(TARGET_LINUX_ARM64)
	cross build --release --target $(TARGET_DARWIN_AMD64)
	cross build --release --target $(TARGET_DARWIN_ARM64)
	cp target/$(TARGET_LINUX_AMD64)/release/$(BINARY_NAME) dist/$(BINARY_NAME)-linux-amd64
	cp target/$(TARGET_LINUX_ARM64)/release/$(BINARY_NAME) dist/$(BINARY_NAME)-linux-arm64
	cp target/$(TARGET_DARWIN_AMD64)/release/$(BINARY_NAME) dist/$(BINARY_NAME)-darwin-amd64
	cp target/$(TARGET_DARWIN_ARM64)/release/$(BINARY_NAME) dist/$(BINARY_NAME)-darwin-arm64
	# Generate checksums
	@cd dist && ( \
		sha256sum $(BINARY_NAME)-linux-amd64 $(BINARY_NAME)-linux-arm64 $(BINARY_NAME)-darwin-amd64 $(BINARY_NAME)-darwin-arm64; \
		sha256sum ../install.sh | sed 's#  \.\./install\.sh$$#  install.sh#'; \
	) > checksums.txt
	# Write version file
	@echo "$(VERSION)" > dist/latest
	@echo "Release binaries built in dist/"

## upload: Upload release binaries and install script to Cloudflare R2
BINARIES := \
	dist/$(BINARY_NAME)-linux-amd64 dist/$(BINARY_NAME)-linux-arm64 \
	dist/$(BINARY_NAME)-darwin-amd64 dist/$(BINARY_NAME)-darwin-arm64

upload: release
	@echo "Uploading $(VERSION) to R2:$(R2_BUCKET)..."
	@for bin in $(BINARIES); do \
		name=$$(basename $$bin); \
		echo "Uploading $$name..."; \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/$$name --file "$$bin"; \
	done
	bunx wrangler r2 object put --remote $(R2_BUCKET)/checksums.txt --file dist/checksums.txt
	bunx wrangler r2 object put --remote $(R2_BUCKET)/latest --file dist/latest
	bunx wrangler r2 object put --remote $(R2_BUCKET)/install.sh --file install.sh
	@echo "Upload complete!"
	@echo "Install with: curl -fsSL https://get.lukan.ai/install.sh | bash"

## upload-gh: Upload release binaries to GitHub Releases (creates or overwrites)
GH_REPO ?= lukanlabs/lukan
upload-gh: release
	@echo "Uploading $(VERSION) to GitHub Releases..."
	@if gh release view $(VERSION) --repo $(GH_REPO) >/dev/null 2>&1; then \
		echo "Release $(VERSION) exists, overwriting assets..."; \
		gh release upload $(VERSION) --repo $(GH_REPO) --clobber \
			$(BINARIES) dist/checksums.txt dist/latest install.sh; \
	else \
		echo "Creating release $(VERSION)..."; \
		gh release create $(VERSION) --repo $(GH_REPO) \
			--title "$(VERSION)" \
			--notes "Release $(VERSION)" \
			$(BINARIES) dist/checksums.txt dist/latest install.sh; \
	fi
	@echo "Upload complete!"
	@echo "https://github.com/$(GH_REPO)/releases/tag/$(VERSION)"

## help: Show this help
help:
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@sed -n 's/^## //p' $(MAKEFILE_LIST) | column -t -s ':' | sed 's/^/  /'
