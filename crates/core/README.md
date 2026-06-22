# clise

Core library crate for parsing, formatting, editing, and validating structured data files (JSON, JSONC, YAML, TOML) using JSON Schemas.

## Key Features

- **Multi-Format Support**: Unified abstraction for JSON, JSONC, YAML, and TOML.
- **JSON Schema Validation**: Schema-guided auto-completion, structural validation, and type inference.
- **Comment-Preserving Edits**: Modify configurations programmatically without stripping existing comments or formatting.
- **Undo / Redo History**: Integrated state tracking for structural edits.

## Quick Start

Add this to your `Cargo.toml`:
```toml
[dependencies]
clise = "0.1.1"
serde_json = "1.0"
```

Then, use `EditorState` to load and mutate configurations:
```rust
use clise::prelude::*;
use serde_json::json;

fn main() {
    let data = json!({
        "project": "clise",
        "version": "0.1.0"
    });

    // Initialize state
    let mut state = EditorState::new(data, Format::Json, None, None);

    // Save history and mutate
    state.save_to_undo();
    if let Some(version_val) = state.data.pointer_mut("/version") {
        *version_val = serde_json::Value::String("0.2.0".to_string());
    }
    state.rebuild_flattened();

    // Serialize changes
    state.on_save();
    let serialized = clise::format::serialize(&state.data, Format::Json, None, false).unwrap();
    println!("{}", serialized);
}
```

## Documentation

For more detailed API and library specifications, please refer to the [Reference Guide](https://github.com/0deep/clise/blob/main/docs/reference.md).

