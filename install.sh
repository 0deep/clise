#!/bin/sh
# clise one-line installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/0deep/clise/main/install.sh | sh

{ # Prevent execution of incomplete script due to download interruption

set -eu

# --- Single Point of Truth: Configuration ---
# Override via environment variables:
#   CLISE_VERSION      - Install specific version (e.g., v0.3.4)
#   CLISE_INSTALL_DIR  - Custom install directory (default: /usr/local/bin)
#   GITHUB_REPOSITORY  - Override repo (default: 0deep/clise)
REPO="${GITHUB_REPOSITORY:-0deep/clise}"
BINARY_NAME="clise"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${CLISE_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
COMP_DIR_BASH="/etc/bash_completion.d"
COMP_DIR_ZSH="/usr/local/share/zsh/site-functions"

# --- Output formatting helpers (modern, minimal ANSI colors) ---
# Disable colors automatically when output is not a TTY or NO_COLOR is set.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_CYAN='\033[36m'
    C_GREEN='\033[32m'
    C_YELLOW='\033[33m'
    C_RED='\033[31m'
    C_BOLD='\033[1m'
    C_DIM='\033[2m'
    C_RESET='\033[0m'
else
    C_CYAN=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_BOLD=''; C_DIM=''; C_RESET=''
fi

clise_step() {
    printf "  ${C_CYAN}•${C_RESET} %s\n" "$1"
}

clise_ok() {
    printf "  ${C_GREEN}✓${C_RESET} %s\n" "$1"
}

clise_warn() {
    printf "  ${C_YELLOW}!${C_RESET} %s\n" "$1"
}

clise_err() {
    printf "  ${C_RED}✗${C_RESET} %s\n" "$1" >&2
}

clise_info() {
    printf "    ${C_DIM}%s${C_RESET}\n" "$1"
}

check_cmd() {
    command -v "$1" > /dev/null 2>&1
}

# --- CLI argument parsing ---
show_help() {
    cat <<EOF
clise installer

Usage: install.sh [OPTIONS]

Options:
  -h, --help           Print this help message
  -v, --version <TAG>  Install specific version (e.g., v0.3.4)
  -d, --dir <PATH>     Install directory (default: /usr/local/bin)
  --no-path            Skip shell profile PATH modification

Environment Variables:
  CLISE_VERSION        Override version to install
  CLISE_INSTALL_DIR    Override install directory
  NO_COLOR             Disable colored output
EOF
}

REQUESTED_VERSION=""
SKIP_PATH=0

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            show_help
            exit 0
            ;;
        -v|--version)
            if [ $# -lt 2 ]; then
                clise_err "--version requires an argument"
                exit 1
            fi
            REQUESTED_VERSION="$2"
            shift 2
            ;;
        --version=*)
            REQUESTED_VERSION="${1#--version=}"
            shift
            ;;
        -d|--dir)
            if [ $# -lt 2 ]; then
                clise_err "--dir requires an argument"
                exit 1
            fi
            INSTALL_DIR="$2"
            shift 2
            ;;
        --dir=*)
            INSTALL_DIR="${1#--dir=}"
            shift
            ;;
        --no-path)
            SKIP_PATH=1
            shift
            ;;
        *)
            clise_err "Unknown option: $1"
            clise_info "Run 'install.sh --help' for usage."
            exit 1
            ;;
    esac
done

# Resolve version: CLI flag > env var > latest
if [ -n "$REQUESTED_VERSION" ]; then
    LATEST_RELEASE="$REQUESTED_VERSION"
elif [ -n "${CLISE_VERSION:-}" ]; then
    LATEST_RELEASE="$CLISE_VERSION"
else
    LATEST_RELEASE=""
fi

# --- Require root (auto-elevate with sudo if not root) ---
if [ "$(id -u)" -ne 0 ]; then
    if ! check_cmd sudo; then
        clise_err "This installer requires root privileges, but 'sudo' was not found."
        clise_info "Please run this script as root."
        exit 1
    fi
    if [ -f "$0" ]; then
        exec sudo "$0" "$@"
    elif [ -f "./install.sh" ]; then
        exec sudo sh ./install.sh "$@"
    else
        TMP_INSTALL=$(mktemp)
        INSTALL_URL="https://raw.githubusercontent.com/$REPO/main/install.sh"
        if check_cmd curl; then
            curl --proto '=https' --tlsv1.2 -sL "$INSTALL_URL" -o "$TMP_INSTALL"
        elif check_cmd wget; then
            wget --https-only --secure-protocol=TLSv1_2 -q -O "$TMP_INSTALL" "$INSTALL_URL"
        else
            clise_err "sudo is required but script could not be downloaded."
            clise_info "Please run: curl -fsSL $INSTALL_URL | sudo sh"
            rm -f "$TMP_INSTALL"
            exit 1
        fi
        chmod +x "$TMP_INSTALL"
        exec sudo sh "$TMP_INSTALL" "$@"
    fi
fi

# Restore original user's HOME when running with sudo (for shell profile access)
if [ -n "${SUDO_USER:-}" ] && [ "$HOME" = "/root" ]; then
    ORIG_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6 || true)
    if [ -n "$ORIG_HOME" ] && [ -d "$ORIG_HOME" ]; then
        HOME="$ORIG_HOME"
    fi
fi

# Downloader with TLS 1.2+, retry, and snap curl fallback to wget (rustup pattern)
downloader() {
    _url="$1"
    _out="$2"

    if check_cmd curl; then
        _curl_path="$(command -v curl)"
        case "$_curl_path" in
            */snap/*)
                if check_cmd wget; then
                    wget --https-only --secure-protocol=TLSv1_2 "$_url" -O "$_out"
                    return $?
                else
                    clise_err "curl installed with snap cannot be used to install clise."
                    clise_err "Please uninstall and reinstall curl with a system package manager."
                    exit 1
                fi
                ;;
        esac
        curl --proto '=https' --tlsv1.2 --retry 3 -C - -fL --progress-bar -o "$_out" "$_url"
        return $?
    elif check_cmd wget; then
        wget --https-only --secure-protocol=TLSv1_2 "$_url" -O "$_out"
        return $?
    else
        clise_err "curl or wget is required to download clise."
        exit 1
    fi
}

# Detect platform (rustup pattern & musl libc detection)
detect_platform() {
    _os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    _arch="$(uname -m)"

    case "$_os" in
        linux)  _os="linux" ;;
        darwin) _os="macos" ;;
        *)
            clise_err "Unsupported OS: $_os"
            exit 1
            ;;
    esac

    case "$_arch" in
        x86_64|amd64)  _arch="amd64" ;;
        arm64|aarch64) _arch="arm64" ;;
        *)
            clise_err "Unsupported architecture: $_arch"
            exit 1
            ;;
    esac

    # musl detection (Alpine Linux)
    if [ "$_os" = "linux" ] && ldd --version 2>&1 | grep -q 'musl'; then
        clise_warn "Alpine Linux (musl) detected. Current release assets may not be compatible."
    fi

    TARGET_OS="$_os"
    TARGET_ARCH="$_arch"
}

detect_platform

# Resolve version (if not already set via CLI/env)
if [ -z "$LATEST_RELEASE" ]; then
    if check_cmd curl; then
        LATEST_RELEASE=$(curl --proto '=https' --tlsv1.2 -sI "https://github.com/$REPO/releases/latest" | grep -i 'location:' | sed -E 's/.*\/tag\/([^[:space:]\r\n]+).*/\1/' || true)
    elif check_cmd wget; then
        LATEST_RELEASE=$(wget --https-only --secure-protocol=TLSv1_2 --max-redirect=0 "https://github.com/$REPO/releases/latest" 2>&1 | grep -i 'Location:' | sed -E 's/.*\/tag\/([^[:space:]\r\n]+).*/\1/' || true)
    fi

    if [ -z "$LATEST_RELEASE" ]; then
        if check_cmd curl; then
            LATEST_RELEASE=$(curl --proto '=https' --tlsv1.2 -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
        elif check_cmd wget; then
            LATEST_RELEASE=$(wget --https-only --secure-protocol=TLSv1_2 -qO- "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
        fi
    fi

    if [ -z "$LATEST_RELEASE" ]; then
        clise_err "Could not fetch latest release version automatically."
        clise_err "Please specify a version with --version <TAG> or CLISE_VERSION env var."
        exit 1
    fi
fi

printf "\n%bInstalling clise %s (%s-%s)...%b\n\n" "$C_BOLD" "$LATEST_RELEASE" "$TARGET_OS" "$TARGET_ARCH" "$C_RESET"

RELEASE_URL="https://github.com/$REPO/releases/download/$LATEST_RELEASE/${BINARY_NAME}-${TARGET_OS}-${TARGET_ARCH}.tar.gz"

# Download and unpack
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

clise_step "Downloading pre-built binary..."
if ! downloader "$RELEASE_URL" "$TMP_DIR/clise.tar.gz"; then
    clise_err "Download failed! Binary may not be built for this release yet."
    clise_info "URL: $RELEASE_URL"
    exit 1
fi

clise_step "Extracting package..."
tar -xzf "$TMP_DIR/clise.tar.gz" -C "$TMP_DIR"

# Install binary
# Clean up existing local installation to prevent PATH shadowing
if [ -n "${SUDO_USER:-}" ]; then
    LOCAL_BIN_DIR="$HOME/.local/bin"
    if [ -f "$LOCAL_BIN_DIR/$BINARY_NAME" ]; then
        clise_info "Removing local installation at $LOCAL_BIN_DIR/$BINARY_NAME to prevent PATH shadowing..."
        rm -f "$LOCAL_BIN_DIR/$BINARY_NAME"
    fi
    if [ -h "$LOCAL_BIN_DIR/se" ] || [ -f "$LOCAL_BIN_DIR/se" ]; then
        rm -f "$LOCAL_BIN_DIR/se"
    fi
    LOCAL_COMP_BASH="$HOME/.local/share/bash-completion/completions/$BINARY_NAME"
    if [ -f "$LOCAL_COMP_BASH" ]; then
        clise_info "Removing local bash completion at $LOCAL_COMP_BASH..."
        rm -f "$LOCAL_COMP_BASH"
    fi
    LOCAL_COMP_ZSH="$HOME/.zsh/completion/_$BINARY_NAME"
    if [ -f "$LOCAL_COMP_ZSH" ]; then
        clise_info "Removing local zsh completion at $LOCAL_COMP_ZSH..."
        rm -f "$LOCAL_COMP_ZSH"
    fi
fi

clise_step "Installing binary to $INSTALL_DIR/$BINARY_NAME..."
mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
ln -sf "$BINARY_NAME" "$INSTALL_DIR/se"

# Shell completion generation
if [ "$SKIP_PATH" -eq 0 ]; then
    CURRENT_SHELL=$(basename "${SHELL:-sh}")

    case "$CURRENT_SHELL" in
        bash)
            mkdir -p "$COMP_DIR_BASH"
            if "$INSTALL_DIR/$BINARY_NAME" generate-completion bash > "$COMP_DIR_BASH/$BINARY_NAME"; then
                clise_step "Installed Bash completion to $COMP_DIR_BASH/$BINARY_NAME"
            else
                clise_warn "Failed to auto-generate Bash completion."
            fi
            ;;
        zsh)
            mkdir -p "$COMP_DIR_ZSH"
            if "$INSTALL_DIR/$BINARY_NAME" generate-completion zsh > "$COMP_DIR_ZSH/_$BINARY_NAME"; then
                clise_step "Installed Zsh completion to $COMP_DIR_ZSH/_$BINARY_NAME"

                ZSH_PROFILE="${ZDOTDIR:-${HOME}}/.zshrc"
                FPATH_LINE="fpath=($COMP_DIR_ZSH \$fpath)"
                COMPINIT_LINE="autoload -Uz compinit && compinit"
                if [ -f "$ZSH_PROFILE" ]; then
                    if ! grep -q "fpath=($COMP_DIR_ZSH" "$ZSH_PROFILE"; then
                        {
                            echo ""
                            echo "# clise zsh completion"
                            echo "$FPATH_LINE"
                            echo "$COMPINIT_LINE"
                        } >> "$ZSH_PROFILE"
                        clise_ok "Zsh completion activated in $ZSH_PROFILE"
                    fi
                fi
            else
                clise_warn "Failed to auto-generate Zsh completion."
            fi
            ;;
        *)
            ;;
    esac
fi

printf "\n%b✓ clise %s installed successfully!%b\n\n" "$C_GREEN$C_BOLD" "$LATEST_RELEASE" "$C_RESET"
printf "%bNext steps:%b\n" "$C_BOLD" "$C_RESET"
printf "  Run '%b%s --help%b' to get started\n\n" "$C_CYAN" "$BINARY_NAME" "$C_RESET"

} # Prevent execution of incomplete script
