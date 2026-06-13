# Reference

This document provides the complete API specifications, module architectures, and development guidelines for the `crates/core` library of `clise` (`clise-core`).

---

## 1. Quick Start Example

Here is a simple example demonstrating how to initialize the `EditorState`, navigate nodes, edit values programmatically, and serialize the data back into formatted JSON text.

```rust
use clise_core::prelude::*;
use serde_json::json;

fn main() {
    // 1. Prepare sample configuration data (JSON format)
    let data = json!({
        "project": "clise",
        "version": "0.1.0",
        "settings": {
            "theme": "dark",
            "show_hints": true
        }
    });

    // 2. Initialize the EditorState
    let mut state = EditorState::new(
        data,
        Format::Json,
        Some("config.json".to_string()),
        None, // Raw original text (used for comment preservation)
    );

    // Verify initial layout (root node selected by default)
    println!("Initial selection: {}", state.selected_node().unwrap().key); // Output: "project" or root key

    // 3. Move the cursor down and modify a value
    navigate::move_down(&mut state); // Moves to "project" key
    navigate::move_down(&mut state); // Moves to "version" key

    // Record the change history for Undo
    state.save_to_undo();

    // Mutate the inner JSON tree
    if let Some(version_val) = state.data.pointer_mut("/version") {
        *version_val = serde_json::Value::String("0.2.0".to_string());
    }

    // Rebuild the flattened UI representation after modifying the tree structure/values
    state.rebuild_flattened();

    // 4. Save and serialize changes
    state.on_save();
    let serialized = clise_core::format::serialize(&state.data, Format::Json, None, false).unwrap();
    println!("Serialized Config:\n{}", serialized);
}
```

---

## 2. Module Specifications

### `clise_core::prelude`

Exports the most common structures, enums, and submodules for external integration:
```rust
pub mod prelude {
    pub use crate::state::{EditorState, UiNode, NodeType, EditMode, SchemaState};
    pub use crate::action::Action;
    pub use crate::format::Format;
    pub use crate::theme::Theme;
    pub use crate::render::SchemaEditor;
    pub use crate::navigate;
    #[cfg(feature = "schema")]
    pub use crate::config::CliseConfig;
}
```

---

### `state` Module

Contains the definition of the editor's core runtime state machine and data nodes.

#### `NodeType` (Enum)
Represents the structural type of a JSON tree node.
```rust
pub enum NodeType {
    /// JSON Object container with count of child properties
    Object { child_count: usize },
    /// JSON Array container with count of elements
    Array { child_count: usize },
    /// Leaf node (String, Number, Bool, Null)
    Leaf,
}
```

#### `ValueType` (Enum)
Leaf primitive type used primarily for styling and visual rendering.
```rust
pub enum ValueType {
    Null,
    Bool,
    Number,
    String,
    Object,
    Array,
}
```

#### `UiNode` (Struct)
A flattened node representation calculated to map tree depths into vertical index lists on the terminal display.
```rust
pub struct UiNode {
    pub path: Vec<String>,
    pub depth: usize,
    pub key: String,
    pub value_display: String,
    pub value_type: ValueType,
    pub node_type: NodeType,
    pub expanded: bool,
}
```

#### `EditMode` (Enum)
Represents active UI states, including user inputs, dropdown prompts, search mode, or save dialogues.
```rust
pub enum EditMode {
    Normal,
    Dropdown { options: Vec<String>, selected: usize },
    TextPrompt { buffer: String, cursor_pos: usize },
    NewKeyDropdown { parent_path: Vec<String>, temp_key: String, options: Vec<String>, selected: usize },
    NewKeyPrompt { parent_path: Vec<String>, temp_key: String, buffer: String, cursor_pos: usize },
    RenameKeyPrompt { parent_path: Vec<String>, original_key: String, buffer: String, cursor_pos: usize, value: serde_json::Value },
    SavePrompt { selected: usize },
    SearchPrompt { buffer: String, cursor_pos: usize },
    Help,
}
```

#### `EditorState` (Struct)
Main state struct maintaining the original JSON structure, current editor mode, selected line index, history stack, and search status.
- **Methods**:
  - `pub fn new(data: Value, format: Format, filename: Option<String>, original_text: Option<String>) -> Self`: Creates a new editor state.
  - `pub fn scroll_viewport(&mut self, delta: isize)`: Scroll viewport by delta lines without moving the cursor.
  - `pub fn node_index_at_y(&self, y: usize) -> Option<usize>`: Gets the index of the node displayed at the given screen y position.
  - `pub fn is_node_modified(&self, path: &[String]) -> bool`: Compares the node with the original data to check if modified.
  - `pub fn on_save(&mut self)`: Backs up the current data and resets dirty/changed flags.
  - `pub fn set_status(&mut self, message: String)`: Sets a temporary status message to render.
  - `pub fn save_to_undo(&mut self)`: Pushes current data and status to the undo stack.
  - `pub fn pop_undo(&mut self)`: Pops and discards the top undo state.
  - `pub fn undo(&mut self)`: Reverts to the previous history state.
  - `pub fn redo(&mut self)`: Reapplies a previously undone change.
  - `pub fn delete_node(&mut self, path: &[String]) -> Result<(), String>`: Deletes the node at the specified path.
  - `pub fn add_child_node(&mut self, parent_path: &[String], key: Option<String>, value: Value) -> Result<(), String>`: Appends a child to a parent JSON Object or Array.
  - `pub fn move_node_up(&mut self)`: Reorders a property or element upwards.
  - `pub fn move_node_down(&mut self)`: Reorders a property or element downwards.
  - `pub fn perform_search(&mut self, query: &str)`: Runs full matching query across keys and values.
  - `pub fn perform_search_realtime(&mut self, query: &str)`: Runs realtime search highlighting matches.
  - `pub fn data(&self) -> &Value`: Immutable access to the working JSON Value.
  - `pub fn selected_node(&self) -> Option<&UiNode>`: Retrieves the currently selected flattened node.
  - `pub fn get_completions_at_cursor(&self) -> Vec<CompletionItem>`: Fetches completion candidates for the current selected path.
  - `pub fn get_completions_for_path(&self, path: &[String]) -> Vec<CompletionItem>`: Fetches completion candidates for any JSON path.
  - `pub fn apply_completion(&mut self, path: &[String], item: &CompletionItem)`: Applies autocomplete item.
  - `pub fn auto_adjust_expansion(&mut self, height: usize)`: Expands nodes automatically to fit the screen size.
  - `pub fn handle_key_event(&mut self, event: crossterm::event::KeyEvent) -> Action`: Directs keystrokes to either navigate or edit states, returning an `Action` requested to the host program.

---

### `action` Module

Provides signals returned from the editor widget to let the CLI or host application execute OS/IO operations.

#### `Action` (Enum)
```rust
pub enum Action {
    Noop,
    RequestSchemaFetch { filename: String },
    Save { data: Value, format: Format },
    SaveAndQuit { data: Value, format: Format },
    Quit,
}
```

---

### `format` Module

Handles deserialization (parsing) and serialization (formatting/saving) while preserving file-specific configurations and comments.

#### `Format` (Enum)
```rust
pub enum Format {
    Json,
    Jsonc, // JSON with comments
    Yaml,
    Toml,
}
```

#### `FormatError` (Enum)
Wraps underlying format parser/serializer errors (`serde_json`, `serde_saphyr`, `toml`, `toml_edit`).

- **Functions**:
  - `pub fn parse(input: &str, format: Format) -> Result<Value, FormatError>`: Parses text source into JSON Value.
  - `pub fn serialize(value: &Value, format: Format, original_text: Option<&str>, key_order_changed: bool) -> Result<String, FormatError>`: Formats JSON Value into target string, maintaining comments if `original_text` is supplied.
  - `pub fn serialize_with_renames(value: &Value, format: Format, original_text: Option<&str>, key_order_changed: bool, renamed_keys: &HashMap<String, String>) -> Result<String, FormatError>`: Serializes preserving comment nodes, with custom key rename overrides mapping.

---

### `theme` Module

Manages styles and colors applied to TUI display nodes.

#### `Theme` (Struct)
Defines customizable styles based on `ratatui::style::Style`:
```rust
pub struct Theme {
    pub key_style: Style,
    pub string_style: Style,
    pub number_style: Style,
    pub bool_style: Style,
    pub null_style: Style,
    pub bracket_style: Style,
    pub focused_style: Style,
    pub status_style: Style,
    pub indent_guide_style: Style,
    pub error_style: Style,
}
```
*Implements `Default` providing a pre-configured Catppuccin Mocha-inspired palette.*

---

### `render` Module

Integrates with Ratatui to render TUI layouts.

#### `SchemaEditor` (Struct)
Implements `ratatui::widgets::StatefulWidget` for `EditorState`.
- **Methods**:
  - `pub fn new(theme: &'a Theme) -> Self`: Constructs the widget reference.
  - `pub fn block(mut self, block: Block<'a>) -> Self`: Wraps the widget with a Ratatui Block border.

---

### `navigate` Module

Stateless helper functions for modifying selection ranges and viewport settings in `EditorState`.

- **Functions**:
  - `pub fn move_up(state: &mut EditorState)`: Moves selection up.
  - `pub fn move_down(state: &mut EditorState)`: Moves selection down.
  - `pub fn page_up(state: &mut EditorState)`: Moves selection up by viewport height.
  - `pub fn page_down(state: &mut EditorState)`: Moves selection down by viewport height.
  - `pub fn toggle_expand(state: &mut EditorState)`: Toggles expansion of objects/arrays.
  - `pub fn expand_or_move_to_last_child(state: &mut EditorState)`: Expands a node, or jumps to its last nested element.
  - `pub fn collapse_current(state: &mut EditorState)`: Collapses a node, or jumps to its parent.
  - `pub fn ensure_visible(state: &mut EditorState, viewport_height: usize)`: Centers scroll position around the cursor selection.

---

### `edit` Module

Facilitates mutating configurations and resolving JSON Schemas.

- **Functions**:
  - `pub fn start_edit(state: &mut EditorState)`: Enters edit mode on selected node.
  - `pub fn start_edit_cleared(state: &mut EditorState)`: Enters edit mode, clearing the previous value buffer.
  - `pub fn find_sub_schema<'a>(schema: &'a Value, path: &[String]) -> Option<&'a Value>`: Resolves schema attributes for a nested JSON pointer path.
  - `pub fn get_completions_for_path(state: &EditorState, path: &[String]) -> Vec<CompletionItem>`: Suggests autocompletion properties.
  - `pub fn apply_completion(state: &mut EditorState, path: &[String], item: &CompletionItem)`: Applies suggestions to state tree.
  - `pub fn resolve_schema_type_and_default(root: &Value, current: &Value) -> (Option<Value>, Option<String>)`: Extracts defaults and constraints.
  - `pub fn apply_edit(state: &mut EditorState)`: Persists buffered text/dropdown selection into state value tree.
  - `pub fn cancel_edit(state: &mut EditorState)`: Discards active prompt buffer and returns to normal navigation.
  - `pub fn get_addable_keys(state: &EditorState, path: &[String]) -> Vec<String>`: Identifies schema-defined keys not currently defined in object.
  - `pub fn trigger_add_child(state: &mut EditorState)`: Initiates dropdown/prompt to insert a new property.

---

### `flatten` Module

Processes visual list calculation mapping.

- **Functions**:
  - `pub fn rebuild_flattened(data: &Value, prev_nodes: &[UiNode], show_child_counts: bool, schema: Option<&Value>) -> Vec<UiNode>`: Calculates vector of visible lines based on tree nodes expanded states.
  - `pub fn count_nodes_per_level(value: &Value, depth: usize, counts: &mut Vec<usize>)`: Traverses sizes of arrays and objects.

---

### `schema` Module (Conditional: `#[cfg(feature = "schema")]`)

Enables JSON Schema validation catalog resolving.

#### `SchemaError` (Enum)
Wraps catalog IO / cache directory / HTTP client errors.

#### `SchemaCatalog` / `SchemaEntry` (Structs)
Map items from SchemaStore metadata listings.

#### `SchemaFetcher` (Struct)
Manages caching and HTTP resolution of schema files.
- **Methods**:
  - `pub fn new() -> Result<Self, SchemaError>`: Connects project local cache directory path.
  - `pub async fn fetch_catalog(&self) -> Result<SchemaCatalog, SchemaError>`: Fetches official registry of schemas.
  - `pub async fn fetch_schema(&self, url: &str) -> Result<Value, SchemaError>`: Resolves schema definitions (checks Cache first, updates if elder than 30 days).

---

### `config` Module (Conditional: `#[cfg(feature = "schema")]`)

Controls local mappings.

#### `SchemaMapping` (Struct)
Binds glob file filters to exact Schema URL routes.

#### `CliseConfig` (Struct)
Handles `schemas.json` configuration file read/writes.
- **Methods**:
  - `pub fn load_or_init() -> Self`: Loads user definitions.
  - `pub fn save(&self) -> std::io::Result<()>`: Saves mapping.
  - `pub fn config_path() -> PathBuf`: Path to config file.
  - `pub fn update_mapping(&mut self, file: String, url: String, name: String, downloaded: bool)`: Insert/updates schemas mappings.
  - `pub fn get_mapping(&self, file: &str) -> Option<SchemaMapping>`: Searches mappings for a match using exact name or glob expansion.
