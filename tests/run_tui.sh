#!/usr/bin/env bash
# run_tui.sh - backend-neutral clise TUI test runner
#
# Usage:
#   tests/run_tui.sh [--backend tu] <casefile.case>
#   tests/run_tui.sh [--backend tu] --all <dir>      # All .case files in directory
#
# Contract:
#   - Case definitions (.case) are tool-neutral. Any TUI backend consumes them identically.
#   - Backend implements backend_run() provided by tests/backends/<name>.sh.
#   - backend_run outputs the saved file path to stdout.
#
# Backends other than tu (tmux+expect, etc.) can be added by implementing the same contract
# in tests/backends/<name>.sh.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

BACKEND="tu"
ALL=""
CASE_ARG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --backend) BACKEND="$2"; shift 2 ;;
    --all) ALL=1; CASE_ARG="$2"; shift 2 ;;
    *) CASE_ARG="$1"; shift ;;
  esac
done

if [[ -z "$CASE_ARG" ]]; then
  echo "usage: $0 [--backend tu] <case.case> | --all <dir>" >&2
  exit 1
fi

source "$SCRIPT_DIR/backends/$BACKEND.sh" || {
  echo "backend '$BACKEND' load failed" >&2; exit 1
}

run_one() {
  local casefile="$1"
  parse_case "$casefile"
  echo "=== [${CASE_ID}] ${CASE_TITLE} ==="
  echo "    fixture: ${CASE_FIXTURE}"

  local fixture; fixture="$(resolve_path "$CASE_FIXTURE")"
  local schema=""; [[ -n "$CASE_SCHEMA" ]] && schema="$(resolve_path "$CASE_SCHEMA")"

  local saved
  saved="$(backend_run "$fixture" "$schema" "$CASE_SIZE" "${CASE_KEYS[@]}")"

  if [[ ! -f "$saved" ]]; then
    echo "    FAIL: saved file not found ($saved)"; return 1
  fi

  echo "----- saved -----"
  sed 's/^/    /' "$saved"
  echo "-----------------"

  local rc=0
  assert_case "$saved"; rc=$?

  if [[ $rc -eq 0 ]]; then
    echo "    RESULT: PASS"
  else
    echo "    RESULT: FAIL"
  fi
  rm -rf "$(dirname "$saved")"
  return $rc
}

overall=0
if [[ -n "$ALL" ]]; then
  shopt -s nullglob
  while IFS= read -r -d '' f; do
    run_one "$f" || overall=1
  done < <(find "$CASE_ARG" -name '*.case' -print0)
else
  run_one "$CASE_ARG" || overall=1
fi

exit $overall
