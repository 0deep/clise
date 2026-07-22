#!/bin/sh
# clise one-line installer script
# Usage: curl -fsSL <install_script_url> | sh

{ # Prevent execution of incomplete script due to download interruption

set -e

# --- Output formatting helpers (no emoji, ANSI colors) ---
# Disable colors automatically when output is not a TTY or NO_COLOR is set.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    C_CYAN='\033[1;36m'
    C_BLUE='\033[1;34m'
    C_GREEN='\033[0;32m'
    C_YELLOW='\033[0;33m'
    C_RED='\033[0;31m'
    C_BOLD='\033[1m'
    C_RESET='\033[0m'
else
    C_CYAN=''; C_BLUE=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_BOLD=''; C_RESET=''
fi

clise_box() {
    # Print a titled box. Usage: clise_box "TITLE"
    local title="$1"
    local line="========================================"
    printf " ${C_CYAN}%s${C_RESET}\n" "$line"
    printf " ${C_CYAN}${C_BOLD} %s${C_RESET}\n" "$title"
    printf " ${C_CYAN}%s${C_RESET}\n" "$line"
}
clise_step() {
    # Print a step line. Usage: clise_step "message"
    printf "${C_BLUE}>>${C_RESET} %s\n" "$1"
}
clise_ok() {
    # Print a success line. Usage: clise_ok "message"
    printf "   ${C_GREEN}[OK]${C_RESET} %s\n" "$1"
}
clise_warn() {
    # Print a warning line. Usage: clise_warn "message"
    printf "   ${C_YELLOW}[WARN]${C_RESET} %s\n" "$1"
}
clise_err() {
    # Print an error line to stderr. Usage: clise_err "message"
    printf "   ${C_RED}[ERROR]${C_RESET} %s\n" "$1" >&2
}
clise_info() {
    # Print an indented info line. Usage: clise_info "message"
    printf "   %s\n" "$1"
}

# --- Re-exec as root if not already (triggers password prompt) ---
if [ "$(id -u)" -ne 0 ]; then
    # When piped (curl|sh, cat|sh), $0 is not a real file and stdin is
    # already consumed by the { } block, so we re-download the script.
    if [ -f "$0" ]; then
        exec sudo "$0" "$@"
    else
        TMP_INSTALL=$(mktemp)
        INSTALL_URL="https://raw.githubusercontent.com/0deep/clise/main/install.sh"
        if type curl >/dev/null 2>&1; then
            curl -fsSL "$INSTALL_URL" -o "$TMP_INSTALL"
        elif type wget >/dev/null 2>&1; then
            wget -q -O "$TMP_INSTALL" "$INSTALL_URL"
        else
            clise_err "sudo is required but the script could not be re-downloaded."
            clise_info "Please run: curl -fsSL $INSTALL_URL -o install.sh && sudo sh install.sh" >&2
            rm -f "$TMP_INSTALL"
            exit 1
        fi
        chmod +x "$TMP_INSTALL"
        exec sudo sh "$TMP_INSTALL" "$@"
    fi
fi

# --- Configuration ---
OWNER="0deep"  # Replace with actual GitHub owner
REPO="clise"
BINARY_NAME="clise"

# Detect actual user when running with sudo
if [ -n "${SUDO_USER-}" ]; then
    INSTALL_UID="$SUDO_UID"
    INSTALL_GID="${SUDO_GID:-$SUDO_UID}"
    # Restore original user's HOME if it was changed to /root
    if [ "$HOME" = "/root" ]; then
        ORIG_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
        if [ -n "$ORIG_HOME" ] && [ -d "$ORIG_HOME" ]; then
            HOME="$ORIG_HOME"
        fi
    fi
else
    INSTALL_UID=$(id -u)
    INSTALL_GID=$(id -g)
fi

if [ -z "${INSTALL_DIR-}" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
    fi
fi

COMP_DIR_BASH="$HOME/.local/share/bash-completion/completions"
COMP_DIR_ZSH="$HOME/.zsh/completion"

clise_box "clise installing"

# --- Helper Functions ---
clise_has() {
    type "$1" > /dev/null 2>&1
}

clise_download() {
    local URL="$1"
    local OUT="$2"
    if clise_has "curl"; then
        curl -H "Cache-Control: no-cache" -L -f -o "$OUT" "$URL"
    elif clise_has "wget"; then
        wget -q -O "$OUT" "$URL"
    else
        clise_err "curl or wget is required to download clise." >&2
        exit 1
    fi
}

clise_detect_profile() {
    if [ "${PROFILE-}" = '/dev/null' ]; then
        return
    fi
    if [ -n "${PROFILE-}" ] && [ -f "${PROFILE}" ]; then
        echo "${PROFILE}"
        return
    fi

    local DETECTED_PROFILE=""
    local SHELL_NAME
    SHELL_NAME=$(basename "$SHELL")

    case "$SHELL_NAME" in
        bash)
            if [ -f "$HOME/.bashrc" ]; then
                DETECTED_PROFILE="$HOME/.bashrc"
            elif [ -f "$HOME/.bash_profile" ]; then
                DETECTED_PROFILE="$HOME/.bash_profile"
            fi
            ;;
        zsh)
            if [ -f "${ZDOTDIR:-${HOME}}/.zshrc" ]; then
                DETECTED_PROFILE="${ZDOTDIR:-${HOME}}/.zshrc"
            fi
            ;;
    esac

    if [ -z "$DETECTED_PROFILE" ]; then
        for EACH_PROFILE in ".profile" ".bashrc" ".bash_profile" ".zshrc"
        do
            if [ -f "$HOME/$EACH_PROFILE" ]; then
                DETECTED_PROFILE="$HOME/$EACH_PROFILE"
                break
            fi
        done
    fi

    if [ -n "$DETECTED_PROFILE" ]; then
        echo "$DETECTED_PROFILE"
    fi
}

# 1. Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    linux)   TARGET_OS="linux" ;;
    darwin)  TARGET_OS="macos" ;;
    *)       
        clise_err "Unsupported OS: $OS"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)  TARGET_ARCH="amd64" ;;
    arm64|aarch64) TARGET_ARCH="arm64" ;;
    *)       
        clise_err "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# 2. Get latest release version (Try redirect link first to avoid rate limiting)
clise_step "Fetching latest version info..."
LATEST_RELEASE=""
if clise_has "curl"; then
    LATEST_RELEASE=$(curl -sI "https://github.com/$OWNER/$REPO/releases/latest" | grep -i 'location:' | sed -E 's/.*\/tag\/([^[:space:]\r\n]+).*/\1/')
elif clise_has "wget"; then
    LATEST_RELEASE=$(wget --max-redirect=0 "https://github.com/$OWNER/$REPO/releases/latest" 2>&1 | grep -i 'Location:' | sed -E 's/.*\/tag\/([^[:space:]\r\n]+).*/\1/')
fi

# Fallback to API if redirect check failed
if [ -z "$LATEST_RELEASE" ]; then
    if clise_has "curl"; then
        LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$OWNER/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    elif clise_has "wget"; then
        LATEST_RELEASE=$(wget -qO- "https://api.github.com/repos/$OWNER/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    fi
fi

if [ -z "$LATEST_RELEASE" ]; then
    LATEST_RELEASE="v0.3.3"
    clise_warn "Could not fetch latest release automatically. Falling back to $LATEST_RELEASE"
fi

clise_info "Latest Version: $LATEST_RELEASE"
RELEASE_URL="https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/${BINARY_NAME}-${TARGET_OS}-${TARGET_ARCH}.tar.gz"

# 3. Download and unpack
TMP_DIR=$(mktemp -d)
CLEANUP() {
    rm -rf "$TMP_DIR"
}
trap CLEANUP EXIT

clise_step "Downloading pre-built binary for ${TARGET_ARCH}-${TARGET_OS}..."
if ! clise_download "$RELEASE_URL" "$TMP_DIR/clise.tar.gz"; then
    clise_err "Download failed! Binary may not be built for this release yet."
    clise_info "URL: $RELEASE_URL"
    exit 1
fi

clise_step "Extracting package..."
tar -xzf "$TMP_DIR/clise.tar.gz" -C "$TMP_DIR"

# 4. Install binary
# Clean up existing local installation if installing globally with sudo
# to prevent PATH shadowing (where an older local version runs instead of the new global one)
if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER-}" ]; then
    LOCAL_BIN_DIR="$HOME/.local/bin"
    if [ -f "$LOCAL_BIN_DIR/$BINARY_NAME" ]; then
        clise_info "Found existing local installation at $LOCAL_BIN_DIR/$BINARY_NAME. Removing to prevent PATH shadowing..."
        rm -f "$LOCAL_BIN_DIR/$BINARY_NAME"
    fi
    if [ -h "$LOCAL_BIN_DIR/se" ] || [ -f "$LOCAL_BIN_DIR/se" ]; then
        rm -f "$LOCAL_BIN_DIR/se"
    fi
fi

mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
chown "$INSTALL_UID:$INSTALL_GID" "$INSTALL_DIR/$BINARY_NAME" 2>/dev/null || true
ln -sf "$BINARY_NAME" "$INSTALL_DIR/se"
chown -h "$INSTALL_UID:$INSTALL_GID" "$INSTALL_DIR/se" 2>/dev/null || true
clise_ok "Installed binary to $INSTALL_DIR/$BINARY_NAME"
clise_ok "Created symbolic link 'se' -> '$BINARY_NAME' in $INSTALL_DIR"

# 5. Generate and install shell completions automatically
clise_step "Generating and installing shell completions..."

CURRENT_SHELL=$(basename "$SHELL")

case "$CURRENT_SHELL" in
    bash)
        mkdir -p "$COMP_DIR_BASH"
        if "$INSTALL_DIR/$BINARY_NAME" generate-completion bash > "$COMP_DIR_BASH/$BINARY_NAME" 2>/dev/null; then
            chown "$INSTALL_UID:$INSTALL_GID" "$COMP_DIR_BASH/$BINARY_NAME" 2>/dev/null || true
            clise_ok "Bash completion installed to $COMP_DIR_BASH/$BINARY_NAME"
        else
            clise_warn "Failed to auto-generate Bash completion."
        fi
        ;;
    zsh)
        mkdir -p "$COMP_DIR_ZSH"
        if "$INSTALL_DIR/$BINARY_NAME" generate-completion zsh > "$COMP_DIR_ZSH/_$BINARY_NAME" 2>/dev/null; then
            chown "$INSTALL_UID:$INSTALL_GID" "$COMP_DIR_ZSH/_$BINARY_NAME" 2>/dev/null || true
            clise_ok "Zsh completion installed to $COMP_DIR_ZSH/_$BINARY_NAME"

            # Auto-activate completion in zsh profile (idempotent)
            ZSH_PROFILE="${ZDOTDIR:-${HOME}}/.zshrc"
            FPATH_LINE="fpath=($COMP_DIR_ZSH \$fpath)"
            COMPINIT_LINE="autoload -Uz compinit && compinit"
            if [ -f "$ZSH_PROFILE" ]; then
                if ! grep -q "fpath=($COMP_DIR_ZSH" "$ZSH_PROFILE" 2>/dev/null; then
                    echo "" >> "$ZSH_PROFILE"
                    echo "# clise zsh completion" >> "$ZSH_PROFILE"
                    echo "$FPATH_LINE" >> "$ZSH_PROFILE"
                    echo "$COMPINIT_LINE" >> "$ZSH_PROFILE"
                    clise_ok "Zsh completion activated in $ZSH_PROFILE (restart shell or run 'autoload -Uz compinit && compinit')"
                else
                    clise_ok "Zsh completion already activated in $ZSH_PROFILE"
                fi
            else
                clise_info "To enable completions, add the following to ~/.zshrc:"
                clise_info "$FPATH_LINE"
                clise_info "$COMPINIT_LINE"
            fi
        else
            clise_warn "Failed to auto-generate Zsh completion."
        fi
        ;;
    *)
        clise_warn "Auto-completions are not supported for shell: $CURRENT_SHELL."
        clise_info "You can generate them manually via: $BINARY_NAME generate-completion <SHELL>"
        ;;
esac

# 6. Path check and final instructions
if [ "$INSTALL_DIR" != "/usr/local/bin" ] && [ "$INSTALL_DIR" != "/usr/bin" ]; then
    USER_PROFILE=$(clise_detect_profile)
    PATH_STR="export PATH=\"\$PATH:$INSTALL_DIR\""

    case :$PATH: in
        *:$INSTALL_DIR:*) ;;
        *)
            if [ -n "$USER_PROFILE" ]; then
                if ! grep -qc "$INSTALL_DIR" "$USER_PROFILE" 2>/dev/null; then
                    clise_step "Appending PATH configuration to $USER_PROFILE"
                    echo "" >> "$USER_PROFILE"
                    echo "# clise path configuration" >> "$USER_PROFILE"
                    echo "$PATH_STR" >> "$USER_PROFILE"
                else
                    clise_ok "clise PATH configuration already in $USER_PROFILE"
                fi
            else
                clise_warn "$INSTALL_DIR is not in your PATH."
                clise_info "Please add the following line to your shell configuration (~/.bashrc or ~/.zshrc):"
                clise_info "$PATH_STR"
            fi
            ;;
    esac
fi

clise_box "clise installed"

} # Prevent execution of incomplete script

