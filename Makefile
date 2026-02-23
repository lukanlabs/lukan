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

.PHONY: all build clean test check install release bundle-plugins package-plugins upload upload-gh help

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
	@# Google Workspace plugin
	@cd plugins/google-workspace/dist && tar czf ../../../dist/plugins/lukan-plugin-google-workspace.tar.gz .
	@echo "  Packaged: lukan-plugin-google-workspace.tar.gz"

## release: Build binary + bundle plugins + generate checksums
release: build bundle-plugins package-plugins
	@echo "Building release $(VERSION) for $(PLATFORM)..."
	@mkdir -p dist
	@cp target/release/$(BINARY_NAME) dist/$(BINARY_NAME)-$(PLATFORM)
	@# Generate checksums
	@cd dist && sha256sum $(BINARY_NAME)-$(PLATFORM) > checksums.txt
	@cd dist && sha256sum ../install.sh | sed 's#  \.\./install\.sh$$#  install.sh#' >> checksums.txt
	@cd dist/plugins && sha256sum *.tar.gz >> ../checksums.txt
	@# Write version file
	@echo "$(VERSION)" > dist/latest
	@echo ""
	@echo "Release $(VERSION) built in dist/"
	@echo "  Binary:  dist/$(BINARY_NAME)-$(PLATFORM)"
	@echo "  Plugins: dist/plugins/*.tar.gz"
	@echo "  Checksums: dist/checksums.txt"

## upload: Upload release to Cloudflare R2
upload: release
	@echo "Uploading $(VERSION) to R2:$(R2_BUCKET)..."
	bunx wrangler r2 object put --remote $(R2_BUCKET)/$(BINARY_NAME)-$(PLATFORM) --file dist/$(BINARY_NAME)-$(PLATFORM)
	bunx wrangler r2 object put --remote $(R2_BUCKET)/checksums.txt --file dist/checksums.txt
	bunx wrangler r2 object put --remote $(R2_BUCKET)/latest --file dist/latest
	bunx wrangler r2 object put --remote $(R2_BUCKET)/install.sh --file install.sh
	@# Upload plugins
	@for f in dist/plugins/*.tar.gz; do \
		name=$$(basename $$f); \
		echo "Uploading plugin: $$name"; \
		bunx wrangler r2 object put --remote $(R2_BUCKET)/plugins/$$name --file "$$f"; \
	done
	@# Upload registry
	bunx wrangler r2 object put --remote $(R2_BUCKET)/registry.toml --file registry.toml
	@echo ""
	@echo "Upload complete!"
	@echo "Install: curl -fsSL https://get.lukan.ai/install.sh | bash"

## upload-gh: Upload release to GitHub Releases
upload-gh: release
	@echo "Uploading $(VERSION) to GitHub Releases..."
	@if gh release view $(VERSION) --repo $(GH_REPO) >/dev/null 2>&1; then \
		echo "Release $(VERSION) exists, overwriting assets..."; \
		gh release upload $(VERSION) --repo $(GH_REPO) --clobber \
			dist/$(BINARY_NAME)-$(PLATFORM) dist/checksums.txt dist/latest install.sh \
			dist/plugins/*.tar.gz; \
	else \
		echo "Creating release $(VERSION)..."; \
		gh release create $(VERSION) --repo $(GH_REPO) \
			--title "$(VERSION)" \
			--notes "Release $(VERSION)" \
			dist/$(BINARY_NAME)-$(PLATFORM) dist/checksums.txt dist/latest install.sh \
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
