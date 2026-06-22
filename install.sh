#!/bin/sh
# clise one-line installer script
# Usage: curl -fsSL <install_script_url> | sh

{ # Prevent execution of incomplete script due to download interruption

set -e

# --- Configuration ---
OWNER="0deep"  # Replace with actual GitHub owner
REPO="clise"
BINARY_NAME="clise"

if [ -z "${INSTALL_DIR-}" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="$HOME/.local/bin"
    fi
fi

COMP_DIR_BASH="$HOME/.local/share/bash-completion/completions"
COMP_DIR_ZSH="$HOME/.zsh/completion"

echo "=== clise installer ==="

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
        echo "❌ Error: curl or wget is required to download clise." >&2
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
        echo "❌ Unsupported OS: $OS"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)  TARGET_ARCH="amd64" ;;
    arm64|aarch64) TARGET_ARCH="arm64" ;;
    *)       
        echo "❌ Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# 2. Get latest release version (Try redirect link first to avoid rate limiting)
echo "🔍 Fetching latest version info..."
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
    LATEST_RELEASE="v0.1.0"
    echo "⚠️ Could not fetch latest release automatically. Falling back to $LATEST_RELEASE"
fi

echo "🚀 Latest Version: $LATEST_RELEASE"
RELEASE_URL="https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/${BINARY_NAME}-${TARGET_OS}-${TARGET_ARCH}.tar.gz"

# 3. Download and unpack
TMP_DIR=$(mktemp -d)
CLEANUP() {
    rm -rf "$TMP_DIR"
}
trap CLEANUP EXIT

echo "📥 Downloading pre-built binary for ${TARGET_ARCH}-${TARGET_OS}..."
if ! clise_download "$RELEASE_URL" "$TMP_DIR/clise.tar.gz"; then
    echo "❌ Download failed! Binary may not be built for this release yet."
    echo "URL: $RELEASE_URL"
    exit 1
fi

echo "📦 Extracting package..."
tar -xzf "$TMP_DIR/clise.tar.gz" -C "$TMP_DIR"

# 4. Install binary
mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"
ln -sf "$BINARY_NAME" "$INSTALL_DIR/se"
echo "✅ Installed binary successfully to $INSTALL_DIR/$BINARY_NAME"
echo "🔗 Created symbolic link 'se' -> '$BINARY_NAME' in $INSTALL_DIR"

# 5. Generate and install shell completions automatically
echo "⚙️ Generating and installing shell completions..."

CURRENT_SHELL=$(basename "$SHELL")

case "$CURRENT_SHELL" in
    bash)
        mkdir -p "$COMP_DIR_BASH"
        if "$INSTALL_DIR/$BINARY_NAME" generate-completion bash > "$COMP_DIR_BASH/$BINARY_NAME" 2>/dev/null; then
            echo "✅ Bash completion auto-installed to $COMP_DIR_BASH/$BINARY_NAME"
        else
            echo "⚠️ Failed to auto-generate Bash completion."
        fi
        ;;
    zsh)
        mkdir -p "$COMP_DIR_ZSH"
        if "$INSTALL_DIR/$BINARY_NAME" generate-completion zsh > "$COMP_DIR_ZSH/_$BINARY_NAME" 2>/dev/null; then
            echo "✅ Zsh completion auto-installed to $COMP_DIR_ZSH/_$BINARY_NAME"
            
            # Setup path hint
            if ! grep -q "fpath=(.*$COMP_DIR_ZSH" ~/.zshrc 2>/dev/null; then
                echo "💡 Zsh 사용자 안내: ~/.zshrc 파일에 다음 설정을 추가하여 자동완성을 활성화하세요:"
                echo "   fpath=($COMP_DIR_ZSH \$fpath)"
                echo "   autoload -U compinit && compinit"
            fi
        else
            echo "⚠️ Failed to auto-generate Zsh completion."
        fi
        ;;
    *)
        echo "ℹ️ Auto-completions are not supported for shell: $CURRENT_SHELL. You can generate them manually via '$BINARY_NAME generate-completion <SHELL>'."
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
                    echo "=> Appending PATH configuration to $USER_PROFILE"
                    echo "" >> "$USER_PROFILE"
                    echo "# clise path configuration" >> "$USER_PROFILE"
                    echo "$PATH_STR" >> "$USER_PROFILE"
                else
                    echo "=> clise PATH configuration already in $USER_PROFILE"
                fi
            else
                echo "⚠️  WARNING: $INSTALL_DIR is not in your PATH."
                echo "   Please add the following line to your shell configuration (~/.bashrc or ~/.zshrc):"
                echo "   $PATH_STR"
            fi
            ;;
    esac
fi

echo "🎉 Installation completed successfully!"

} # Prevent execution of incomplete script

