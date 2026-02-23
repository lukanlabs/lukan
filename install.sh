#!/usr/bin/env bash
set -euo pipefail

# Parse command line arguments
TARGET="${1:-}"

# Validate target if provided
if [[ -n "${TARGET}" ]] && [[ ! "${TARGET}" =~ ^(stable|latest|v?[0-9]+\.[0-9]+\.[0-9]+(-[^[:space:]]+)?)$ ]]; then
  echo "Usage: $0 [stable|latest|VERSION]" >&2
  exit 1
fi

# --- Download sources (primary + fallback) ---
PRIMARY_URL="https://get.lukan.ai"
GH_REPO="lukanlabs/lukan"
FALLBACK_URL="https://github.com/${GH_REPO}/releases/latest/download"

BIN_DIR="${HOME}/.local/bin"
DOWNLOAD_DIR=$(mktemp -d)
SOURCE=""

info() { printf "\033[36m%s\033[0m\n" "$*"; }
ok()   { printf "\033[32m✓\033[0m %s\n" "$*"; }
warn() { printf "\033[33m⚠\033[0m %s\n" "$*"; }
err()  { printf "\033[31m✗\033[0m %s\n" "$*" >&2; }

cleanup() { rm -rf "${DOWNLOAD_DIR}"; }
trap cleanup EXIT

# --- Check for required dependencies ---
DOWNLOADER=""
if command -v curl >/dev/null 2>&1; then
  DOWNLOADER="curl"
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER="wget"
else
  err "Either curl or wget is required but neither is installed"
  exit 1
fi

download() {
  local url="$1"
  local output="${2:-}"
  local show_progress="${3:-}"
  if [ "${DOWNLOADER}" = "curl" ]; then
    if [ -n "${output}" ]; then
      if [ "${show_progress}" = "1" ]; then
        curl -fL --progress-bar --connect-timeout 10 --max-time 120 --retry 3 --retry-delay 2 -o "${output}" "${url}"
      else
        curl -fsSL --connect-timeout 10 --max-time 60 --retry 3 --retry-delay 2 -o "${output}" "${url}"
      fi
    else
      curl -fsSL --connect-timeout 10 --max-time 60 --retry 3 --retry-delay 2 "${url}"
    fi
  else
    if [ -n "${output}" ]; then
      if [ "${show_progress}" = "1" ]; then
        wget --show-progress -q --timeout=30 --tries=3 --waitretry=2 -O "${output}" "${url}"
      else
        wget -q --timeout=30 --tries=3 --waitretry=2 -O "${output}" "${url}"
      fi
    else
      wget -q --timeout=30 --tries=3 --waitretry=2 -O - "${url}"
    fi
  fi
}

# --- Detect OS ---
case "$(uname -s)" in
  Darwin)  OS="darwin" ;;
  Linux)   OS="linux" ;;
  MINGW*|MSYS*|CYGWIN*)
    err "Windows is not supported. Use WSL (Windows Subsystem for Linux) instead."
    exit 1
    ;;
  *)
    err "Unsupported OS: $(uname -s)"
    exit 1
    ;;
esac

# --- Detect architecture ---
case "$(uname -m)" in
  x86_64|amd64)  ARCH="amd64" ;;
  aarch64|arm64) ARCH="arm64" ;;
  *)             err "Unsupported architecture: $(uname -m)"; exit 1 ;;
esac

# --- Detect Rosetta 2 on macOS ---
if [ "${OS}" = "darwin" ] && [ "${ARCH}" = "amd64" ]; then
  if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = "1" ]; then
    ARCH="arm64"
    info "Rosetta 2 detected, using native arm64 binary"
  fi
fi

# --- Check for musl on Linux ---
if [ "${OS}" = "linux" ]; then
  if [ -f /lib/libc.musl-x86_64.so.1 ] || [ -f /lib/libc.musl-aarch64.so.1 ] || ldd /bin/ls 2>&1 | grep -q musl; then
    err "musl-based Linux (Alpine) is not currently supported"
    err "Use a glibc-based distribution (Ubuntu, Debian, Fedora, etc.)"
    exit 1
  fi
fi

PLATFORM="${OS}-${ARCH}"
info "Detected: ${PLATFORM}"

# --- GitHub Releases download helper (uses gh CLI for private repos) ---
gh_download() {
  local asset_name="$1"
  local output="$2"
  gh release download --repo "${GH_REPO}" --pattern "${asset_name}" --dir "${DOWNLOAD_DIR}" --clobber 2>/dev/null
  if [ -n "${output}" ] && [ -f "${DOWNLOAD_DIR}/${asset_name}" ] && [ "${DOWNLOAD_DIR}/${asset_name}" != "${output}" ]; then
    mv "${DOWNLOAD_DIR}/${asset_name}" "${output}"
  fi
}

# --- Resolve download source: try primary, fallback to GitHub Releases ---
resolve_source() {
  # Try primary
  CHECKSUMS=$(download "${PRIMARY_URL}/checksums.txt" 2>/dev/null || echo "")
  if [ -n "${CHECKSUMS}" ]; then
    SOURCE="primary"
    BASE_URL="${PRIMARY_URL}"
    info "Downloading from primary CDN"
    return
  fi

  # Fallback to GitHub Releases (requires gh CLI for private repos)
  warn "Primary CDN unavailable, falling back to GitHub Releases..."
  if ! command -v gh >/dev/null 2>&1; then
    err "GitHub CLI (gh) is required for fallback downloads. Install it: https://cli.github.com"
    exit 1
  fi

  gh_download "checksums.txt" "${DOWNLOAD_DIR}/checksums.txt"
  if [ -f "${DOWNLOAD_DIR}/checksums.txt" ]; then
    CHECKSUMS=$(cat "${DOWNLOAD_DIR}/checksums.txt")
  fi

  if [ -n "${CHECKSUMS}" ]; then
    SOURCE="github"
    info "Downloading from GitHub Releases"
    return
  fi

  err "Could not download checksums.txt from any source"
  exit 1
}

resolve_source

# --- Fetch version ---
if [ -n "${TARGET}" ] && [ "${TARGET}" != "stable" ] && [ "${TARGET}" != "latest" ]; then
  VERSION="${TARGET}"
  info "Requested version: ${VERSION}"
else
  if [ "${SOURCE}" = "github" ]; then
    gh_download "latest" "${DOWNLOAD_DIR}/latest"
    VERSION=$(cat "${DOWNLOAD_DIR}/latest" 2>/dev/null || echo "")
  else
    VERSION=$(download "${BASE_URL}/latest" 2>/dev/null || echo "")
  fi
  if [ -n "${VERSION}" ]; then
    VERSION=$(echo "${VERSION}" | tr -d '[:space:]')
    info "Latest version: ${VERSION}"
  fi
fi

# --- Download, verify and install a binary ---
# Usage: install_binary <remote_name> <local_name>
install_binary() {
  local REMOTE_NAME="$1"
  local LOCAL_NAME="$2"
  local BINARY_PATH="${DOWNLOAD_DIR}/${REMOTE_NAME}"

  info "Downloading ${LOCAL_NAME}..."

  if [ "${SOURCE}" = "primary" ]; then
    if ! download "${BASE_URL}/${REMOTE_NAME}" "${BINARY_PATH}" 1; then
      err "Download failed: ${REMOTE_NAME}"
      return 1
    fi
  else
    gh_download "${REMOTE_NAME}" "${BINARY_PATH}"
    if [ ! -f "${BINARY_PATH}" ]; then
      err "Download failed: ${REMOTE_NAME}"
      return 1
    fi
  fi

  # Verify checksum
  EXPECTED=$(echo "${CHECKSUMS}" | grep "  ${REMOTE_NAME}$" | awk '{print $1}')
  if [ -z "${EXPECTED}" ] || [[ ! "${EXPECTED}" =~ ^[a-f0-9]{64}$ ]]; then
    err "No valid checksum found for ${REMOTE_NAME}"
    return 1
  fi

  if [ "${OS}" = "darwin" ]; then
    ACTUAL=$(shasum -a 256 "${BINARY_PATH}" | cut -d' ' -f1)
  else
    ACTUAL=$(sha256sum "${BINARY_PATH}" | cut -d' ' -f1)
  fi

  if [ "${ACTUAL}" != "${EXPECTED}" ]; then
    err "Checksum verification failed for ${LOCAL_NAME}"
    err "Expected: ${EXPECTED}"
    err "Actual:   ${ACTUAL}"
    return 1
  fi

  # Install (rm first to handle "Text file busy" when binary is running)
  rm -f "${BIN_DIR}/${LOCAL_NAME}"
  cp "${BINARY_PATH}" "${BIN_DIR}/${LOCAL_NAME}"
  chmod +x "${BIN_DIR}/${LOCAL_NAME}"
  ok "Installed ${LOCAL_NAME}"
}

# --- Install binaries ---
mkdir -p "${BIN_DIR}"

install_binary "lukan-${PLATFORM}" "lukan"

ok "Checksum verified"

# --- Setup PATH ---
if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
  SHELL_NAME="$(basename "${SHELL:-bash}")"
  case "${SHELL_NAME}" in
    zsh)  RC_FILE="${HOME}/.zshrc" ;;
    bash) RC_FILE="${HOME}/.bashrc" ;;
    fish) RC_FILE="${HOME}/.config/fish/config.fish" ;;
    *)    RC_FILE="${HOME}/.profile" ;;
  esac

  if [ "${SHELL_NAME}" = "fish" ]; then
    LINE="set -gx PATH ${BIN_DIR} \$PATH"
  else
    LINE="export PATH=\"${BIN_DIR}:\$PATH\""
  fi

  if ! grep -qF "${BIN_DIR}" "${RC_FILE}" 2>/dev/null; then
    echo "" >> "${RC_FILE}"
    echo "# lukan" >> "${RC_FILE}"
    echo "${LINE}" >> "${RC_FILE}"
    ok "Added ${BIN_DIR} to PATH in ${RC_FILE}"
  fi

  printf "\n\033[33mRestart your shell or run:\033[0m\n"
  printf "  source %s\n\n" "${RC_FILE}"
fi

# --- Done ---
echo ""
echo "Installation complete!"
echo ""
echo "Get started:"
echo "  lukan setup           # Configure provider & API key"
echo "  lukan                 # Start interactive chat"
echo ""
