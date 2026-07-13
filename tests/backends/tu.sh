#!/usr/bin/env bash
# backends/tu.sh - tu (headless virtual terminal) backend
# backend_run <fixture_abs> <schema_abs|""> <size> <keys...>
#   => Outputs the saved (modified) file path (HOST) as the last line of stdout
#
# Tool-neutral contract:
#   - Copy fixture/schema to a temporary directory within container mount area (/work = PROJECT_DIR)
#     (container cannot see host /tmp)
#   - tu run is invoked with container path (/work/...)
#   - After key transmission, guarantee save and output HOST path
#   - Temporary directory cleanup is handled by the runner (run_tui.sh)

BIN_HOST="$PROJECT_DIR/target/debug/clise-cli"
TU_IMG="localhost/terminal-use:latest"
SESSION="clise-tui-test"

backend_run() {
  local fixture="$1" schema="$2" size="${3:-140x40}"; shift 3
  local keys=("$@")

  if [[ ! -f "$BIN_HOST" ]]; then
    echo ">>> clise-cli not found. Building..." >&2
    (cd "$PROJECT_DIR" && ./dev.sh build)
  fi

  # tu-runner container (project mount)
  if [[ -z "$(docker ps -q --filter name=tu-runner 2>/dev/null)" ]]; then
    echo ">>> Starting tu-runner" >&2
    docker run -d -v "$PROJECT_DIR":/work -w /work --name tu-runner "$TU_IMG" >/dev/null
  fi

  # Contamination-prevention copy: temporary directory within PROJECT_DIR (visible as /work in container)
  local tmpd; tmpd="$(mktemp -d -p "$PROJECT_DIR" clise_XXXXXX)"
  local base; base="$(basename "$tmpd")"
  local ext="${fixture##*.}"
  local work_host="$tmpd/clise.$ext"
  local work_ct="/work/$base/clise.$ext"
  cp "$fixture" "$work_host"

  local schema_arg_ct=()
  local schema_port=""
  if [[ -n "$schema" && -f "$schema" ]]; then
    cp "$schema" "$tmpd/schema.json"
    # clise --schema requires http(s) absolute URL → serve via in-container temporary HTTP server
    schema_port=$((8000 + ($$ % 1000)))
    docker exec -d tu-runner python3 -m http.server "$schema_port" --directory "/work/$base" >/dev/null 2>&1
    sleep 0.5
    schema_arg_ct=(--schema "http://localhost:$schema_port/schema.json")
  fi

  # Spawn (ignore retry if already running) — tu flags (--size/--name) must precede the command
  docker exec tu-runner tu run --size "$size" --name "$SESSION" \
    /work/target/debug/clise-cli "$work_ct" "${schema_arg_ct[@]}" >/dev/null 2>&1 || true
  docker exec tu-runner tu run --size "$size" --name "$SESSION" \
    /work/target/debug/clise-cli "$work_ct" "${schema_arg_ct[@]}" >/dev/null 2>&1

  # Key transmission
  for k in "${keys[@]}"; do
    if [[ "$k" == type:* ]]; then
      docker exec tu-runner tu type "${k#type:}" --name "$SESSION" >/dev/null 2>&1 || true
    else
      docker exec tu-runner tu press "$k" --name "$SESSION" >/dev/null 2>&1 || true
    fi
    sleep 0.15
  done
  docker exec tu-runner tu wait --stable 300 --name "$SESSION" >/dev/null 2>&1 || true

  # Exit (handle dirty prompt: q y)
  docker exec tu-runner tu press q y --name "$SESSION" >/dev/null 2>&1 || true
  sleep 0.3
  docker exec tu-runner tu kill --name "$SESSION" >/dev/null 2>&1 || true

  # Schema HTTP server cleanup
  if [[ -n "$schema_port" ]]; then
    docker exec tu-runner sh -c "pkill -f 'http.server $schema_port'" >/dev/null 2>&1 || true
  fi

  # Output HOST path (runner asserts then cleans up)
  echo "$work_host"
}
