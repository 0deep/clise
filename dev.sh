#!/usr/bin/env bash
set -euo pipefail

# Dev build/test/security audit script
# Usage: ./dev.sh [build|test|audit|lint|clippy|all|shell]

IMAGE="localhost/rust-dev:latest"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

mkdir -p "${SCRIPT_DIR}/target"

RELEASE_FLAG=""

RUN_OPTS=(
  --rm
  -v "${SCRIPT_DIR}:/app"
  -v "${SCRIPT_DIR}/target:/app/target"
  -v "rust-cargo-cache:/usr/local/cargo/registry"
  -w /app
)

run() {
  docker run "${RUN_OPTS[@]}" "${IMAGE}" "$@"
}

do_build() {
  echo ">>> Building${RELEASE_FLAG:+ (release)}..."
  run bash -c "apt-get update -qq && apt-get install -y -qq pkg-config libfontconfig-dev >/dev/null 2>&1; cargo build${RELEASE_FLAG} --workspace --all-targets"
  local profile="debug"
  [[ -n "$RELEASE_FLAG" ]] && profile="release"
  echo ">>> Build complete: target/${profile}/"
}

do_test() {
  echo ">>> Testing${RELEASE_FLAG:+ (release)}..."
  run bash -c "apt-get update -qq && apt-get install -y -qq pkg-config libfontconfig-dev >/dev/null 2>&1; cargo test${RELEASE_FLAG} --all-features"
  echo ">>> Tests complete"
}

do_audit() {
  echo ">>> Security audit..."
  run bash -c 'apt-get update -qq && apt-get install -y -qq pkg-config libfontconfig-dev >/dev/null 2>&1; cargo install cargo-audit --locked; cargo audit'
  echo ">>> Audit complete"
}

do_lint() {
  echo ">>> Linting..."
  run bash -c 'apt-get update -qq && apt-get install -y -qq pkg-config libfontconfig-dev >/dev/null 2>&1; cargo install cargo-deny --locked; cargo deny check'
  echo ">>> Lint complete"
}

do_clippy() {
  echo ">>> Running clippy..."
  run bash -c "apt-get update -qq && apt-get install -y -qq pkg-config libfontconfig-dev >/dev/null 2>&1; cargo clippy${RELEASE_FLAG} --workspace --all-targets -- -D warnings"
  echo ">>> Clippy complete"
}

do_fmt() {
  echo ">>> Formatting check..."
  run bash -c "cargo fmt --check"
  echo ">>> Fmt check complete"
}

do_all() {
  do_fmt
  do_build
  do_clippy
  do_test
  do_lint
  do_audit
  echo ">>> All checks complete!"
}

do_shell() {
  echo ">>> Entering container shell (exit: Ctrl+D)"
  docker run "${RUN_OPTS[@]}" -it "${IMAGE}" bash
}

usage() {
  cat <<EOF
Usage: $0 <command> [--release]

Commands:
  build   Build project
  test    Run tests
  audit   Security audit (cargo-audit)
  lint    Lint (cargo-deny)
  clippy  Lint (cargo-clippy)
  fmt     Check formatting (cargo fmt --check)
  all     Full validation (fmt + build + clippy + test + audit)
  shell   Enter container shell

Options:
  --release   Build/test in release mode
EOF
}

if [[ $# -eq 0 ]]; then
  usage
  exit 1
fi

COMMAND="$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) RELEASE_FLAG="--release"; shift ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
done

case "$COMMAND" in
  build)  do_build ;;
  test)   do_test ;;
  audit)  do_audit ;;
  lint)   do_lint ;;
  clippy) do_clippy ;;
  fmt)    do_fmt ;;
  all)    do_all ;;
  shell)  do_shell ;;
  *)      usage; exit 1 ;;
esac