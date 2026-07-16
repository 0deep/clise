# clise TUI Test Suite (tool-neutral / backend-swappable)

Tool-neutral case definitions + `tu` backend runner verification, executable on **any TUI testing tool**
for array/object/leaf/TUI global features.

## Structure
```
tests/
├── README.md                 # This document
├── lib.sh                    # Case (.case) parser + assertions (tool-neutral)
├── run_tui.sh                # Universal runner (backend loading, case execution/assertion)
├── backends/
│   └── tu.sh                 # tu backend (backend_run implementation)
├── cases/                    # Tool-neutral case definitions (.case) — consumable by any tool
│   ├── array/*.case
│   ├── object/*.case
│   ├── leaf/*.case
│   └── tui/*.case
└── fixtures/                 # Pre-built files + schemas
```

## Design Principles: Separating Test Definitions from Backends
- **Case definitions (`.case`) are tool-neutral.** Only fixture path + key sequence + expected values are specified.
  Consumable identically by tools other than `tu` (tmux+expect, termina, custom harness, etc.).
- **Backends** only need to implement one `backend_run()` function provided by `tests/backends/<name>.sh`
  to be swappable. The runner is not dependent on any specific backend.

### Backend Contract
```sh
backend_run <fixture_abs_path> <schema_abs_path|""> <size> <keys...>
  → Outputs the HOST path of the saved (modified) file as the last line of stdout
```
Backend responsibilities:
1. Copy fixture to prevent contamination (within container mount area).
2. Key transmission (typing uses `type:` prefix token).
3. Guarantee save (`s` or `q y`).
4. Output HOST path. Temporary directory cleanup is handled by the runner via `dirname`.

Adding a new backend: implement `backend_run` with the above contract in `tests/backends/<name>.sh`,
then run with `run_tui.sh --backend <name> <case>`.

## `.case` Format
```
id: C2.1
title: Comment first array item then save/verify
fixture: fixtures/c2_1.yaml
schema: fixtures/schema.json      # Leave empty if none
size: 140x40
keys: Right Down / s              # Space-separated tokens. type:WORD is character input
expect_contains:                 # Each line must be present in saved file (indentation ignored)
  # - apple
  banana
expect_not_contains:
  - 2
expect_exact:                    # Entire block must be present in saved file (partial match)
  a: 1
  # b: 2
```
Key tokens: `Up Down Left Right Enter Esc Backspace Delete d s q y / Ctrl+Up Ctrl+Down Alt+Up Alt+Down
PageUp PageDown f t k ?`. Character input uses `type:<text>`.

## Execution
```sh
# All (25 cases)
tests/run_tui.sh --all tests/cases
# Single
tests/run_tui.sh tests/cases/array/c2_1.case
# With different backend (e.g., tmux)
tests/run_tui.sh --backend tmux tests/cases/array/c2_1.case
```
On first run, `./dev.sh build` (rust-dev container) and `tu-runner` (terminal-use) are started automatically.
Cases requiring a schema are served via an in-container temporary HTTP server (`clise --schema` only supports http(s) URLs).

## Verification Method
After each case ends, the saved file is asserted via `read` (runner uses grep-based assertions).
Pass criteria: saved file structure matches the case's `expect_*` fields.

## Regression Mapping (Bug → Case → Protection Code)
Cases and protection code locations that prevent recurrence of fixed bugs.

### Array Nodes
| Bug | Case | Code |
|---|---|---|
| Multiple comment skip | C2.2 | state.rs:2982 |
| Last item comment failure | C2.3 | state.rs:3023 |
| Duplicate on uncomment | C2.4 | state.rs:3051 |
| Move disabled item (no data loss) | C4.3 | state.rs:928 |
| Delete disabled item removes it | C5.3 | state.rs:798 |
| Array delete renumber + value sync | C5.1 | state.rs:820 |
| Leading commented item add index collision | C3.3 | state.rs:844 |

### Object Nodes
| Bug | Case | Code |
|---|---|---|
| Middle key comment reorder | O5.2 | state.rs:2970 |
| Comment active key then delete removes it | O3.2 | state.rs:798 |
| Delete disabled key removes it | O3.3 | state.rs:798 |
| Re-add key value edit targets active node | O3.4 | node.rs:65 |
| Duplicate key rejection | O-DUP1 | edit.rs:455 |

### Leaf Values
| Bug | Case | Code |
|---|---|---|
| Bool instant toggle (no prompt) | L3.1 | edit.rs:126 |
| Ambiguous string quoting | L1.4 | edit.rs:178 |
| oneOf dropdown only on null | L6.2 | edit.rs:67 |
| Enum priority | L4.3 | edit.rs:139 |
| Ambiguous string quote display | L8.2 | edit.rs:178 |
| Quotes → forced string | L8.4 | edit.rs:711 |
| Schema string preservation | L8.9 | edit.rs:719 |
| []/{} literal type change | L8.8 | edit.rs:715 |

### TUI Global
| Bug | Case | Code |
|---|---|---|
| Save prompt y/n/c | F5.1-5.3 | state.rs:1556 |
| Schema dropdown fallback | F4.3 | edit.rs:921 |
| Undo selection restore | F6.4 | state.rs:2143 |

## Behavior Notes
- When adding an array node, it does not automatically enter value editing prompt (only key is added, value is null).
  This is the expected behavior, so test assertions check key addition only.
- Top-level (root) arrays are also serialized as Sequence like nested arrays (verified by `cases/array/c9_toparray.case`).
- Commented (disabled) nodes CAN now be deleted (`d`) and moved (`Alt+Up/Down`); previously these were no-ops.
  Deleting a disabled array item renumbers siblings and syncs the parent value up the ancestor chain.
- After delete + re-add of the same key, an inactive tombstone coexists with the new active node.
  Path-based lookups (`find_node_by_path`) prefer the ACTIVE node so value edits are not lost to the tombstone.
- `expect_exact` matching matches the entire block exactly as a single substring (preserving lines, indentation, and order).
