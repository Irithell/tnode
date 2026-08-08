#!/usr/bin/env bash
set -euo pipefail

REPO="Irithell/tnode"
RELEASES_URL="https://github.com/${REPO}/releases"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info()    { echo -e "${CYAN}[tnode]${RESET} $*"; }
success() { echo -e "${GREEN}[tnode]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[tnode]${RESET} $*"; }
error()   { echo -e "${RED}[tnode] error:${RESET} $*" >&2; exit 1; }

INSTALL_VERSION=""
INSTALL_PREFIX=""
NO_VERIFY=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)   INSTALL_VERSION="$2"; shift 2 ;;
        --prefix)    INSTALL_PREFIX="$2";  shift 2 ;;
        --no-verify) NO_VERIFY=1;          shift   ;;
        -h|--help)
            echo "Usage: install.sh [--version v0.1.0] [--prefix /path] [--no-verify]"
            exit 0
            ;;
        *) error "unknown argument: $1" ;;
    esac
done

detect_arch() {
    case "$(uname -m)" in
        aarch64|arm64)    echo "aarch64" ;;
        x86_64|amd64)     echo "x86_64"  ;;
        armv7l|armv7|arm) echo "armv7"   ;;
        i686|i386|x86)    echo "i686"    ;;
        *) error "unsupported architecture: $(uname -m). TNode builds are provided for aarch64, x86_64, armv7, and i686." ;;
    esac
}

detect_platform() {
    case "$(uname -s)" in
        Linux)
            if [[ -d "/data/data/com.termux" ]] || [[ -n "${TERMUX_VERSION:-}" ]]; then
                echo "android"
            else
                echo "linux"
            fi
            ;;
        Darwin)
            error "macOS is not supported because LD_PRELOAD is restricted by macOS SIP. TNode is designed for Termux on Android and Linux."
            ;;
        *) error "unsupported platform: $(uname -s)" ;;
    esac
}

need_cmd() {
    command -v "$1" &>/dev/null || error "required command not found: $1"
}

need_cmd curl
need_cmd mktemp

if [[ $NO_VERIFY -eq 0 ]]; then
    if ! command -v sha256sum &>/dev/null && ! command -v shasum &>/dev/null; then
        warn "sha256sum not found — checksum verification disabled"
        NO_VERIFY=1
    fi
fi

ARCH="$(detect_arch)"
PLATFORM="$(detect_platform)"

info "detected platform: ${PLATFORM}/${ARCH}"

if [[ -z "$INSTALL_PREFIX" ]]; then
    if [[ "$PLATFORM" == "android" ]]; then
        INSTALL_PREFIX="${PREFIX:-/data/data/com.termux/files/usr}"
    else
        INSTALL_PREFIX="${HOME}/.local"
    fi
fi

BIN_DIR="${INSTALL_PREFIX}/bin"
LIB_DIR="${INSTALL_PREFIX}/lib"

if [[ -z "$INSTALL_VERSION" ]]; then
    info "fetching latest version tag..."
    INSTALL_VERSION="$(
        curl -fsSL --retry 3 \
            "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' || true
    )"
    [[ -z "$INSTALL_VERSION" ]] && error "could not resolve latest version from GitHub API. Use --version to specify one (e.g. --version v0.1.0)."
fi

info "installing version: ${BOLD}${INSTALL_VERSION}${RESET}"

BINARY_ASSET="tnode-${ARCH}-${PLATFORM}"
LIB_ASSET="libthook-${ARCH}-${PLATFORM}.so"
BASE_URL="${RELEASES_URL}/download/${INSTALL_VERSION}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

download() {
    local url="$1" dest="$2" label="$3"
    info "downloading ${label}..."
    curl -fsSL --retry 3 --progress-bar -o "$dest" "$url" \
        || error "failed to download: ${url}"
}

download "${BASE_URL}/${BINARY_ASSET}" "${TMP_DIR}/tnode"       "tnode binary"
download "${BASE_URL}/${LIB_ASSET}"    "${TMP_DIR}/libthook.so" "libthook library"

if [[ $NO_VERIFY -eq 0 ]]; then
    info "verifying checksums..."
    download "${BASE_URL}/checksums.sha256" "${TMP_DIR}/checksums.sha256" "checksums"

    for file in "tnode:${BINARY_ASSET}" "libthook.so:${LIB_ASSET}"; do
        local_name="${file%%:*}"
        asset_name="${file##*:}"

        expected="$(grep "${asset_name}" "${TMP_DIR}/checksums.sha256" | awk '{print $1}')"
        [[ -z "$expected" ]] && { warn "no checksum found for ${asset_name}, skipping"; continue; }

        if command -v sha256sum &>/dev/null; then
            actual="$(sha256sum "${TMP_DIR}/${local_name}" | awk '{print $1}')"
        else
            actual="$(shasum -a 256 "${TMP_DIR}/${local_name}" | awk '{print $1}')"
        fi

        [[ "$expected" != "$actual" ]] && error "checksum mismatch for ${local_name}
  expected: ${expected}
  got:      ${actual}"
    done

    success "checksums verified successfully"
fi

info "installing binaries to ${INSTALL_PREFIX}..."

mkdir -p "$BIN_DIR" "$LIB_DIR"
install -m 755 "${TMP_DIR}/tnode"       "${BIN_DIR}/tnode"
install -m 644 "${TMP_DIR}/libthook.so" "${LIB_DIR}/libthook.so"

if "${BIN_DIR}/tnode" --tnode-version &>/dev/null; then
    INSTALLED_VERSION="$("${BIN_DIR}/tnode" --tnode-version)"
    success "installation verified: ${INSTALLED_VERSION}"
else
    warn "installed tnode binary, but execution check failed. Make sure ${BIN_DIR} is in your PATH."
fi

if ! echo "$PATH" | grep -q "$BIN_DIR"; then
    echo ""
    warn "${BIN_DIR} is not in your PATH"
    echo "  add to your ~/.zshrc or ~/.bashrc:"
    echo ""
    echo -e "  ${BOLD}export PATH=\"${BIN_DIR}:\$PATH\"${RESET}"
fi

echo ""
echo -e "${BOLD}TNode ${INSTALL_VERSION} installed successfully!${RESET}"
echo ""
echo "  binary    : ${BIN_DIR}/tnode"
echo "  library   : ${LIB_DIR}/libthook.so"
echo ""
echo "  get started:"
echo "    tnode --tnode-doctor"
echo "    tnode --tnode-help"
echo ""
