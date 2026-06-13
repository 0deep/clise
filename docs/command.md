# CLI Command Reference

This document provides a comprehensive specification of the command-line interface, global flags, subcommands, and practical usage examples for the `clise` binary utility.

---

## 1. Global Syntax and Usage

If you run `clise` without a subcommand, it launches the interactive TUI configuration editor in your terminal. You can edit an existing file, or pipe content directly via standard input (stdin).

```bash
clise [FILE] [OPTIONS]

# Pipe empty JSON
echo '{}' | clise --format json

# Pipe JSON with predefined content
echo '{"name": "clise", "version": "0.1.0"}' | clise --format json
```

> [!NOTE]
> If neither `[FILE]` nor piped standard input (stdin) is provided, `clise` will exit immediately with an error message: `Error: No file specified and no stdin provided.`

> [!TIP]
> **Shorthand Command `se`**: The installer creates a symbolic link `se` pointing to `clise` in your PATH. You can substitute `clise` with `se` for all commands (e.g., `se settings.yaml`, `se format Cargo.toml --write`).

### Global Options
- `[FILE]`
  - **Description**: The target file path to open in the TUI editor mode. If omitted and stdin is a pipe, `clise` reads and edits the stdin stream.
- `-f, --format <format>`
  - **Description**: Explicitly specify or override the file format parser.
  - **Allowed Values**: `json`, `jsonc`, `toml`, `yaml` (or `yml`).
- `--schema <schema>`
  - **Description**: Force bind a JSON Schema URL or a local schema file path directly to the editing session.
- `-m, --catalog-match <pattern>`
  - **Description**: Match the file against the schema catalog with a custom filename or glob pattern instead of the actual file path.

---

## 2. Subcommands Specification

### 2.1. `format`
Formats an input configuration file (or standard input stream) and optionally converts it to a different serialization layout.

#### Syntax
```bash
clise format [FILE] [OPTIONS]
```

#### Arguments & Options
- `[FILE]`
  - **Description**: The input file to format. If omitted or passed as `-`, `clise` reads from the standard input stream (stdin).
- `-t, --to <format>`
  - **Description**: Target format to convert the output into.
  - **Allowed Values**: `json`, `jsonc`, `toml`, `yaml`
- `-w, --write`
  - **Description**: Overwrites the target file directly with the formatted text, instead of printing it to standard output (stdout).

#### Examples
```bash
# Formats Cargo.toml in-place
clise format Cargo.toml --write

# Converts a YAML file to prettified JSON format
clise format settings.yaml --to json

# Format JSON string from a curl request via stdin piping
curl -s https://api.github.com/repos/rust-lang/rust | clise format - --to yaml
```

---

### 2.2. `validate`
Validates a target configuration file against a designated JSON Schema.

#### Syntax
```bash
clise validate <FILE> [OPTIONS]
```

#### Arguments & Options
- `<FILE>`
  - **Description**: The target data file to validate. (Required)
- `-s, --schema <schema>`
  - **Description**: Explicit JSON Schema URL or a local file path containing schema rules. If omitted, `clise` searches local mapping tables.
- `-q, --quiet`
  - **Description**: Suppresses verbose error outputs and only returns success (0) or failure (>0) exit codes.
- `-m, --catalog-match <pattern>`
  - **Description**: Match the file against the schema catalog with a custom filename/pattern.

#### Examples
```bash
# Validates package.json against the official npm package schema
clise validate package.json --schema https://json.schemastore.org/package

# Validates config.yaml quietly in a CI/CD script check
if clise validate config.yaml --quiet; then
    echo "Validation succeeded"
else
    echo "Validation failed"
fi
```

---

### 2.3. `schema`
Manages schema maps, glob matches, catalog downloads, and system caches.

#### Syntax
```bash
clise schema <SUBCOMMAND>
```

#### Sub-Subcommands
- `list`
  - **Description**: Displays all registered schema glob pattern mappings.
- `show <file>`
  - **Description**: Resolves and prints the bound schema URL for a specific file name.
- `map <pattern> <schema_url> [OPTIONS]`
  - **Description**: Maps a file glob pattern to a specific JSON Schema URL.
  - **Options**:
    - `-n, --name <name>`: Custom alias/name for the schema mapping.
- `clean`
  - **Description**: Clears all local download caches of JSON Schema files.

#### Examples
```bash
# Register a custom schema mapping for app configuration files
clise schema map "*.config.json" "https://json.schemastore.org/appsettings" --name "AppSettings"

# Show all current configured mappings
clise schema list

# Discover what schema is matching with my tsconfig
clise schema show tsconfig.json

# Clear the cached JSON schema catalog files
clise schema clean
```

---

### 2.4. `init`
Creates a skeleton template file pre-populated with default types and properties parsed from a JSON Schema.

#### Syntax
```bash
clise init <FILE> [OPTIONS]
```

#### Arguments & Options
- `<FILE>`
  - **Description**: The name of the file to create and initialize. (Required)
- `-s, --schema <schema>`
  - **Description**: The schema URL or local file path to extract templates from.
- `-f, --force`
  - **Description**: Overwrites the target file if it already exists.
- `-m, --catalog-match <pattern>`
  - **Description**: Match the file against the schema catalog with a custom filename/pattern.

#### Examples
```bash
# Initialize a new tsconfig.json file prefilled with compilerOptions based on its schema
clise init tsconfig.json --schema https://json.schemastore.org/tsconfig

# Forces initialization of cargo-audit configuration over a pre-existing audit.toml
clise init audit.toml --schema https://json.schemastore.org/cargo-audit -f
```

---

### 2.5. `generate-completion` (Hidden)
Generates shell completion scripts for terminal tab-completion integrations.

#### Syntax
```bash
clise generate-completion <SHELL>
```

#### Arguments
- `<SHELL>`
  - **Description**: The shell system to generate completions for.
  - **Allowed Values**: `bash`, `zsh`, `fish`, `powershell`, `elvish`

#### Examples
```bash
# Load bash completions automatically in current session
source <(clise generate-completion bash)
```
