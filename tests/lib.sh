#!/usr/bin/env bash
# lib.sh - backend-neutral helpers for clise TUI tests
# Shared across any TUI execution backend (tu, tmux+expect, ...).
# Case file (.case) parsing + saved file assertions.
set -uo pipefail
shopt -s extglob

TESTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Workspace root (parent of tests/)
PROJECT_DIR="$(cd "$TESTS_DIR/.." && pwd)"

# ---- .case file parsing -------------------------------------------------------
# Format (tool-neutral, pure bash parsing):
#   id: C2.1
#   title: ...
#   fixture: fixtures/c2_1.yaml
#   schema: fixtures/schema.json      # Leave empty if none
#   size: 140x40
#   keys: Right Down / s              # Space-separated tokens. type:WORD is character input
#   expect_contains:                  # Block (until next KEY:) each line must be in saved file
#     # - apple
#   expect_not_contains:
#     banana
#   expect_exact:                     # Entire block must be in saved file (partial match)
#     ...
parse_case() {
  local file="$1"
  # Store results in global variables
  CASE_ID=""; CASE_TITLE=""; CASE_FIXTURE=""; CASE_SCHEMA=""; CASE_SIZE="140x40"
  CASE_KEYS=(); CASE_CONTAINS=(); CASE_NOT_CONTAINS=(); CASE_EXACT=""
  local in_block="" line k v
  local exact_indent=""
  while IFS= read -r line || [[ -n "$line" ]]; do
    # Currently collecting block
    if [[ -n "$in_block" ]]; then
      if [[ "$line" =~ ^[a-zA-Z_]+: ]]; then
        in_block=""
      else
        if [[ "$in_block" == "expect_exact" ]]; then
          if [[ -z "$exact_indent" ]]; then
            if [[ "$line" =~ ^([[:space:]]+)[^[:space:]] ]]; then
              exact_indent="${BASH_REMATCH[1]}"
            fi
          fi
          local t="$line"
          if [[ -n "$exact_indent" ]]; then
            t="${line#"$exact_indent"}"
          fi
          t="${t%%+([[:space:]])}"
          CASE_EXACT+="$t"$'\n'
          continue
        fi

        # Trim leading/trailing whitespace (ignore indentation), skip empty lines
        local t="${line##+([[:space:]])}"; t="${t%%+([[:space:]])}"
        [[ -z "$t" ]] && continue
        if [[ "$in_block" == "expect_contains" ]]; then
          CASE_CONTAINS+=("$t")
        elif [[ "$in_block" == "expect_not_contains" ]]; then
          CASE_NOT_CONTAINS+=("$t")
        fi
        continue
      fi
    fi
    # KEY: VALUE
    if [[ "$line" =~ ^([a-zA-Z_]+):[[:space:]]*(.*)$ ]]; then
      k="${BASH_REMATCH[1]}"; v="${BASH_REMATCH[2]}"
      case "$k" in
        id) CASE_ID="$v" ;;
        title) CASE_TITLE="$v" ;;
        fixture) CASE_FIXTURE="$v" ;;
        schema) CASE_SCHEMA="$v" ;;
        size) CASE_SIZE="$v" ;;
        keys) read -ra CASE_KEYS <<< "$v" ;;
        expect_contains) in_block="expect_contains" ;;
        expect_not_contains) in_block="expect_not_contains" ;;
        expect_exact) in_block="expect_exact"; exact_indent="" ;;
      esac
    fi
  done < "$file"
}

# ---- Assertions ----------------------------------------------------------------
assert_case() {
  local saved="$1" rc=0 msg
  local content; content="$(cat "$saved" 2>/dev/null || true)"
  local c
  for c in "${CASE_CONTAINS[@]}"; do
    # Skip empty lines
    [[ -z "$c" ]] && continue
    if ! grep -Fq -- "$c" <<<"$content"; then
      echo "    FAIL expect_contains: '$c'"; rc=1
    fi
  done
  for c in "${CASE_NOT_CONTAINS[@]}"; do
    [[ -z "$c" ]] && continue
    if grep -Fq -- "$c" <<<"$content"; then
      echo "    FAIL expect_not_contains: '$c'"; rc=1
    fi
  done
  if [[ -n "$CASE_EXACT" ]]; then
    local trimmed="${CASE_EXACT%$'\n'}"
    if [[ "$content" != *"$trimmed"* ]]; then
      echo "    FAIL expect_exact (block not found)"
      echo "    Expected block:"
      sed 's/^/      /' <<<"$trimmed"
      rc=1
    fi
  fi
  return $rc
}

resolve_path() {
  local p="$1"
  [[ "$p" == /* ]] && { echo "$p"; return; }
  echo "$TESTS_DIR/$p"
}
