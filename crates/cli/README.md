# clise-cli

A terminal-based (TUI) structured data configuration utility powered by JSON Schemas. It provides visual modification and validation for configuration files (JSON, JSONC, YAML, TOML) directly within the command-line interface.

This utility is built on top of the `clise` core library.

## Key Features

- **Interactive TUI**: Navigate and edit complex nested configurations using hierarchical tree views.
- **Auto-completion & Validation**: Powered by JSON Schema fetchers.
- **Multi-Format Integration**: Edit JSON, JSONC, YAML, and TOML seamlessly.
- **Safety**: Integrates schema type guards to prevent invalid types.

## Installation

```bash
cargo install clise-cli
``` 

## Basic Usage

Run without arguments to launch the TUI editor, or pipe structured data via stdin:

```bash
# Launch interactive TUI for a file
clise config.yaml

# Bind a specific JSON Schema
clise settings.json --schema https://json.schemastore.org/package

# Pipe empty JSON directly
echo '{}' | clise --format json
```

## CLI Commands

- **Format**: Prettify or convert configuration files between JSON, YAML, and TOML.
  ```bash
  clise format config.toml --to yaml
  ```
- **Validate**: Check file syntax and structural rules against a JSON Schema.
  ```bash
  clise validate package.json --schema https://json.schemastore.org/package
  ```

## Documentation

For more instructions and command references, please check the following guides:
- [User Guide](https://github.com/0deep/clise/blob/main/docs/guide.md)
- [Command Reference](https://github.com/0deep/clise/blob/main/docs/command.md)

