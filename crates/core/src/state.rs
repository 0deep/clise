use crate::format::Format;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Types of tree nodes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    /// JSON Object (key-value container)
    Object { child_count: usize },
    /// JSON Array (ordered container)
    Array { child_count: usize },
    /// Leaf node (String, Number, Bool, Null)
    Leaf,
}

/// Leaf value types for styling
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    Null,
    Bool,
    Number,
    String,
    Object,
    Array,
}

/// Flattened UI display node
#[derive(Debug, Clone)]
pub struct UiNode {
    /// JSON Pointer path (e.g., ["services", "web", "ports"])
    pub path: Vec<String>,
    /// Indentation depth
    pub depth: usize,
    /// Key name to display
    pub key: String,
    /// Value preview string
    pub value_display: String,
    /// Value type
    pub value_type: ValueType,
    /// Node type
    pub node_type: NodeType,
    /// Expansion state
    pub expanded: bool,
}

/// Helper struct for searching
struct SearchNode {
    path: Vec<String>,
    key: String,
    value: String,
}

/// Edit modes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditMode {
    /// Normal navigation mode
    Normal,
    /// Enum dropdown popup
    Dropdown {
        options: Vec<String>,
        selected: usize,
    },
    /// Text input prompt
    TextPrompt { buffer: String, cursor_pos: usize },
    /// Dropdown for adding a new key (includes parent node path and temporary key)
    NewKeyDropdown {
        parent_path: Vec<String>,
        temp_key: String,
        options: Vec<String>,
        selected: usize,
    },
    /// Text input for adding a new key (includes parent node path and temporary key)
    NewKeyPrompt {
        parent_path: Vec<String>,
        temp_key: String,
        buffer: String,
        cursor_pos: usize,
    },
    /// Rename existing key prompt (parent path, original key name, buffer, cursor position, current value)
    RenameKeyPrompt {
        parent_path: Vec<String>,
        original_key: String,
        buffer: String,
        cursor_pos: usize,
        value: serde_json::Value,
    },
    /// Prompt to confirm saving changes
    SavePrompt { selected: usize },
    /// Search input prompt
    SearchPrompt { buffer: String, cursor_pos: usize },
    /// Help modal
    Help,
}

/// Types of completion suggestion items
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompletionKind {
    /// Object key (Property)
    Property,
    /// Enum value
    Enum,
    /// Boolean value
    Boolean,
    /// Default value
    Default,
}

/// Completion suggestion item
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionItem {
    /// Text to display
    pub label: String,
    /// Actual value to apply (in JSON format)
    pub value: Value,
    /// Suggestion kind
    pub kind: CompletionKind,
    /// Description (Optional)
    pub detail: Option<String>,
}

/// Schema loading state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaState {
    None,
    Loading,
    Loaded,
    Error(String),
}

/// History entry for Undo/Redo
#[derive(Clone)]
struct HistoryEntry {
    data: Value,
    selected: usize,
    original_text: Option<String>,
    key_order_changed: bool,
    renamed_keys: std::collections::HashMap<String, String>,
}

/// Full state of the editor
pub struct EditorState {
    /// Original data tree
    pub data: Value,
    /// Backup of original data tree to detect modifications
    pub original_data: Value,
    /// Flattened view for rendering and cursor navigation
    pub flattened_nodes: Vec<UiNode>,
    /// Cursor position
    pub selected: usize,
    /// Scroll offset
    pub scroll_offset: usize,
    /// Schema loading state
    pub schema_state: SchemaState,
    /// Loaded JSON Schema
    pub schema: Option<Value>,
    /// Current edit mode
    pub edit_mode: EditMode,
    /// Original file format
    pub format: Format,
    /// Filename (used for schema matching)
    pub filename: Option<String>,
    /// Loaded schema name
    pub loaded_schema_name: Option<String>,
    /// Whether to display type hints
    pub show_type_hints: bool,
    /// Whether to display child counts (keys/items)
    pub show_child_counts: bool,
    /// Focus state (useful for mouse focusing in the host app)
    pub focused: bool,
    /// Status message
    pub status_message: Option<(String, std::time::Instant)>,
    /// Undo stack
    undo_stack: Vec<HistoryEntry>,
    /// Redo stack
    redo_stack: Vec<HistoryEntry>,
    /// Whether the data has been modified
    pub is_dirty: bool,
    /// Actual height of the current rendering area (used for PgUp/PgDn scrolling)
    pub viewport_height: usize,
    /// Raw original file content to preserve comments
    pub original_text: Option<String>,
    /// Track if the user explicitly changed the key order (e.g. via Ctrl+Up/Down)
    pub key_order_changed: bool,
    /// Last time the cursor was moved or text was changed (for blink reset)
    pub last_cursor_activity: std::time::Instant,
    /// Current real-time search query
    pub search_query: Option<String>,
    /// Total number of search matches
    pub search_total_matches: usize,
    /// 1-based index of current selected match
    pub search_current_match_index: usize,
    /// Map of renamed keys: key is parent_pointer + "/" + new_key, value is original_key
    pub renamed_keys: std::collections::HashMap<String, String>,
    /// 마우스 호버 중인 노드의 flattened index (None이면 호버 없음)
    pub hovered_node: Option<usize>,
    /// Whether to scroll viewport to keep selected node visible
    pub scroll_to_selected: bool,
    /// Cache of all encountered nodes' expanded states to preserve them even when collapsed
    pub(crate) all_nodes_cache: Vec<UiNode>,
    /// Last time backspace was pressed (to prevent accidental rename prompt)
    pub last_backspace_time: Option<std::time::Instant>,
}

impl EditorState {
    pub fn new(
        data: Value,
        format: Format,
        filename: Option<String>,
        original_text: Option<String>,
    ) -> Self {
        let mut state = Self {
            data: data.clone(),
            original_data: data,
            flattened_nodes: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            schema_state: SchemaState::None,
            schema: None,
            edit_mode: EditMode::Normal,
            format,
            filename,
            loaded_schema_name: None,
            show_type_hints: false,
            show_child_counts: true,
            focused: true,
            status_message: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            is_dirty: false,
            viewport_height: 10,
            original_text,
            key_order_changed: false,
            last_cursor_activity: std::time::Instant::now(),
            search_query: None,
            search_total_matches: 0,
            search_current_match_index: 0,
            renamed_keys: std::collections::HashMap::new(),
            hovered_node: None,
            scroll_to_selected: true,
            all_nodes_cache: Vec::new(),
            last_backspace_time: None,
        };
        state.rebuild_flattened();
        state
    }

    /// Scroll the viewport by delta lines without moving the cursor.
    /// Positive delta = scroll down, negative = scroll up.
    pub fn scroll_viewport(&mut self, delta: isize) {
        self.scroll_to_selected = false;
        let max_scroll = self
            .flattened_nodes
            .len()
            .saturating_sub(self.viewport_height);
        if delta > 0 {
            self.scroll_offset = (self.scroll_offset + delta as usize).min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        }
    }

    /// Given a y offset relative to the list area top, return the flattened node index.
    pub fn node_index_at_y(&self, y: usize) -> Option<usize> {
        let mut current_y: usize = 0;
        let mut idx = self.scroll_offset;
        while idx < self.flattened_nodes.len() {
            let lines = 1; // 비편집 모드에서는 항상 1줄
            if y >= current_y && y < current_y + lines {
                return Some(idx);
            }
            current_y += lines;
            if current_y > self.viewport_height {
                break;
            }
            idx += 1;
        }
        None
    }

    pub fn is_node_modified(&self, path: &[String]) -> bool {
        let pointer = to_json_pointer(path);
        let current_val = match self.data.pointer(&pointer) {
            Some(v) => v,
            None => return false,
        };
        let original_val = match self.original_data.pointer(&pointer) {
            Some(v) => v,
            None => return true,
        };

        match (current_val, original_val) {
            (Value::Object(curr_map), Value::Object(orig_map)) => {
                let curr_keys: Vec<_> = curr_map.keys().collect();
                let orig_keys: Vec<_> = orig_map.keys().collect();
                curr_keys != orig_keys
            }
            (Value::Array(curr_arr), Value::Array(orig_arr)) => curr_arr.len() != orig_arr.len(),
            (curr, orig) => curr != orig,
        }
    }

    pub fn on_save(&mut self) {
        self.original_data = self.data.clone();
        self.is_dirty = false;
        self.key_order_changed = false;
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some((message, std::time::Instant::now()));
    }

    /// Save the current state to the Undo stack
    pub fn save_to_undo(&mut self) {
        self.undo_stack.push(HistoryEntry {
            data: self.data.clone(),
            selected: self.selected,
            original_text: self.original_text.clone(),
            key_order_changed: self.key_order_changed,
            renamed_keys: self.renamed_keys.clone(),
        });
        // Clear the Redo stack when a new operation starts
        self.redo_stack.clear();
        self.is_dirty = true;
    }

    /// Pop the top entry of the Undo stack (used to cancel an incorrectly saved state)
    pub fn pop_undo(&mut self) {
        self.undo_stack.pop();
        if self.undo_stack.is_empty() {
            self.is_dirty = false;
        }
    }

    /// Undo to the previous state
    pub fn undo(&mut self) {
        if let Some(entry) = self.undo_stack.pop() {
            // Save the current state to the Redo stack
            self.redo_stack.push(HistoryEntry {
                data: self.data.clone(),
                selected: self.selected,
                original_text: self.original_text.clone(),
                key_order_changed: self.key_order_changed,
                renamed_keys: self.renamed_keys.clone(),
            });

            let old_data = self.data.clone();

            // Restore state
            self.data = entry.data;
            self.selected = entry.selected;
            self.original_text = entry.original_text;
            self.key_order_changed = entry.key_order_changed;
            self.renamed_keys = entry.renamed_keys;
            self.rebuild_flattened_impl(Some(&old_data));
            self.set_status("Undo".to_string());

            if self.undo_stack.is_empty() {
                self.is_dirty = false;
            }
        }
    }

    /// Redo the next state
    pub fn redo(&mut self) {
        if let Some(entry) = self.redo_stack.pop() {
            // Save the current state to the Undo stack
            self.undo_stack.push(HistoryEntry {
                data: self.data.clone(),
                selected: self.selected,
                original_text: self.original_text.clone(),
                key_order_changed: self.key_order_changed,
                renamed_keys: self.renamed_keys.clone(),
            });

            let old_data = self.data.clone();

            // Restore state
            self.data = entry.data;
            self.selected = entry.selected;
            self.original_text = entry.original_text;
            self.key_order_changed = entry.key_order_changed;
            self.renamed_keys = entry.renamed_keys;
            self.rebuild_flattened_impl(Some(&old_data));
            self.set_status("Redo".to_string());
            self.is_dirty = true;
        }
    }

    pub fn delete_node(&mut self, path: &[String]) -> Result<(), String> {
        self.save_to_undo();
        if path.is_empty() {
            return Err("Cannot delete root node".to_string());
        }

        // Find parent and remove child
        let parent_path = &path[..path.len() - 1];
        let child_key = &path[path.len() - 1];

        let parent_pointer = to_json_pointer(parent_path);
        if let Some(parent) = self.data.pointer_mut(&parent_pointer) {
            match parent {
                Value::Object(map) => {
                    map.remove(child_key);
                }
                Value::Array(arr) => {
                    if let Ok(idx) = child_key.parse::<usize>() {
                        if idx < arr.len() {
                            arr.remove(idx);
                        } else {
                            return Err(format!("Array index out of bounds: {}", idx));
                        }
                    } else {
                        return Err(format!("Invalid array index: {}", child_key));
                    }
                }
                _ => return Err("Parent is not an Object or Array".to_string()),
            }
        } else {
            return Err("Parent node not found".to_string());
        }

        self.all_nodes_cache.retain(|n| !n.path.starts_with(path));
        self.rebuild_flattened();

        // Adjust selected if it's now out of bounds
        if self.selected >= self.flattened_nodes.len() {
            self.selected = self.flattened_nodes.len().saturating_sub(1);
        }

        Ok(())
    }

    pub fn add_child_node(
        &mut self,
        parent_path: &[String],
        key: Option<String>,
        value: Value,
    ) -> Result<(), String> {
        self.save_to_undo();
        let parent_pointer = to_json_pointer(parent_path);
        let mut child_path = parent_path.to_vec();

        if let Some(parent) = self.data.pointer_mut(&parent_pointer) {
            if parent.is_null() {
                let mut initialized = false;
                if let Some(schema) = &self.schema {
                    if let Some(t) = crate::edit::find_sub_schema(schema, parent_path)
                        .and_then(|sub| sub.get("type"))
                        .and_then(|v| v.as_str())
                    {
                        if t == "array" {
                            *parent = Value::Array(Vec::new());
                            initialized = true;
                        } else if t == "object" {
                            *parent = Value::Object(serde_json::Map::new());
                            initialized = true;
                        }
                    }
                }
                if !initialized {
                    if key.is_some() {
                        *parent = Value::Object(serde_json::Map::new());
                    } else {
                        *parent = Value::Array(Vec::new());
                    }
                }
            }

            match parent {
                Value::Object(map) => {
                    let k = key.ok_or_else(|| "Key required for Object".to_string())?;
                    child_path.push(k.clone());
                    map.insert(k, value);
                }
                Value::Array(arr) => {
                    child_path.push(arr.len().to_string());
                    arr.push(value);
                }
                _ => return Err("Parent is not an Object or Array".to_string()),
            }
        } else {
            return Err("Parent node not found".to_string());
        }

        // Set the parent node's expanded state to true
        if let Some(parent_node) = self
            .flattened_nodes
            .iter_mut()
            .find(|n| n.path == parent_path)
        {
            parent_node.expanded = true;
        }

        self.rebuild_flattened();

        // Move the cursor to the newly added child node
        if let Some(pos) = self
            .flattened_nodes
            .iter()
            .position(|n| n.path == child_path)
        {
            self.selected = pos;
        }

        Ok(())
    }

    pub fn move_node_up(&mut self) {
        let node = match self.selected_node() {
            Some(n) => n.clone(),
            None => return,
        };

        if node.path.is_empty() {
            return;
        }

        let parent_path = &node.path[..node.path.len() - 1];
        let child_key = &node.path[node.path.len() - 1];
        let parent_pointer = to_json_pointer(parent_path);

        let mut can_move = false;
        if let Some(parent) = self.data.pointer(&parent_pointer) {
            match parent {
                Value::Array(_) => {
                    if child_key.parse::<usize>().is_ok_and(|idx| idx > 0) {
                        can_move = true;
                    }
                }
                Value::Object(map) => {
                    if map.keys().position(|k| k == child_key).is_some_and(|idx| idx > 0) {
                        can_move = true;
                    }
                }
                _ => {}
            }
        }

        if !can_move {
            return;
        }

        self.save_to_undo();
        let mut new_path = node.path.clone();

        if let Some(parent) = self.data.pointer_mut(&parent_pointer) {
            match parent {
                Value::Array(arr) => {
                    let idx = child_key.parse::<usize>().unwrap();
                    arr.swap(idx, idx - 1);
                    new_path[node.path.len() - 1] = (idx - 1).to_string();
                }
                Value::Object(map) => {
                    let mut items: Vec<(String, serde_json::Value)> =
                        std::mem::take(map).into_iter().collect();
                    let idx = items.iter().position(|(k, _)| k == child_key).unwrap();
                    items.swap(idx, idx - 1);
                    *map = items.into_iter().collect();
                }
                _ => {}
            }
        }

        self.key_order_changed = true;
        self.rebuild_flattened();
        if let Some(pos) = self.flattened_nodes.iter().position(|n| n.path == new_path) {
            self.selected = pos;
        }
    }

    pub fn move_node_down(&mut self) {
        let node = match self.selected_node() {
            Some(n) => n.clone(),
            None => return,
        };

        if node.path.is_empty() {
            return;
        }

        let parent_path = &node.path[..node.path.len() - 1];
        let child_key = &node.path[node.path.len() - 1];
        let parent_pointer = to_json_pointer(parent_path);

        let mut can_move = false;
        if let Some(parent) = self.data.pointer(&parent_pointer) {
            match parent {
                Value::Array(arr) => {
                    if child_key.parse::<usize>().is_ok_and(|idx| idx + 1 < arr.len()) {
                        can_move = true;
                    }
                }
                Value::Object(map) => {
                    if map.keys().position(|k| k == child_key).is_some_and(|idx| idx + 1 < map.len()) {
                        can_move = true;
                    }
                }
                _ => {}
            }
        }

        if !can_move {
            return;
        }

        self.save_to_undo();
        let mut new_path = node.path.clone();

        if let Some(parent) = self.data.pointer_mut(&parent_pointer) {
            match parent {
                Value::Array(arr) => {
                    let idx = child_key.parse::<usize>().unwrap();
                    arr.swap(idx, idx + 1);
                    new_path[node.path.len() - 1] = (idx + 1).to_string();
                }
                Value::Object(map) => {
                    let mut items: Vec<(String, serde_json::Value)> =
                        std::mem::take(map).into_iter().collect();
                    let idx = items.iter().position(|(k, _)| k == child_key).unwrap();
                    items.swap(idx, idx + 1);
                    *map = items.into_iter().collect();
                }
                _ => {}
            }
        }

        self.key_order_changed = true;
        self.rebuild_flattened();
        if let Some(pos) = self.flattened_nodes.iter().position(|n| n.path == new_path) {
            self.selected = pos;
        }
    }

    pub fn perform_search(&mut self, query: &str) {
        if query.is_empty() {
            self.search_total_matches = 0;
            self.search_current_match_index = 0;
            return;
        }

        let query_lower = query.to_lowercase();
        let mut all_nodes = Vec::new();
        self.collect_search_nodes(
            &self.data.clone(),
            Vec::new(),
            "root".to_string(),
            &mut all_nodes,
        );

        // Find matches
        let matches: Vec<_> = all_nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.key.to_lowercase().contains(&query_lower)
                    || node.value.to_lowercase().contains(&query_lower)
            })
            .collect();

        if matches.is_empty() {
            self.search_total_matches = 0;
            self.search_current_match_index = 0;
            self.set_status(format!("No matches for: {}", query));
            return;
        }

        // Find first match after current selected path
        let current_path = self
            .selected_node()
            .map(|n| n.path.clone())
            .unwrap_or_default();
        let current_all_idx = all_nodes
            .iter()
            .position(|n| n.path == current_path)
            .unwrap_or(0);

        let target_match = matches
            .iter()
            .find(|(idx, _)| *idx > current_all_idx)
            .or_else(|| matches.first())
            .map(|(_, node)| node);

        if let Some(target) = target_match {
            // Expand all ancestors
            let mut ancestors = Vec::new();
            for i in 1..target.path.len() {
                ancestors.push(target.path[..i].to_vec());
            }

            for p in ancestors {
                if let Some(node) = self.flattened_nodes.iter_mut().find(|n| n.path == p) {
                    node.expanded = true;
                } else {
                    // Force expansion of previously unflattened node
                    self.flattened_nodes.push(UiNode {
                        path: p,
                        depth: 0,
                        key: String::new(),
                        value_display: String::new(),
                        value_type: ValueType::Null,
                        node_type: NodeType::Leaf, // NodeType doesn't matter for expansion check in rebuild_flattened
                        expanded: true,
                    });
                }
            }

            self.rebuild_flattened();

            // Find new index
            if let Some(pos) = self
                .flattened_nodes
                .iter()
                .position(|n| n.path == target.path)
            {
                self.selected = pos;
                // Center viewport if needed
                if self.selected < self.scroll_offset
                    || self.selected >= self.scroll_offset + self.viewport_height
                {
                    self.scroll_offset = self.selected.saturating_sub(self.viewport_height / 2);
                }
            }
            self.set_status(format!("Found: {}", query));
        }

        self.update_search_match_stats(query);
    }

    fn update_search_match_stats(&mut self, query: &str) {
        if query.is_empty() {
            self.search_total_matches = 0;
            self.search_current_match_index = 0;
            return;
        }

        let query_lower = query.to_lowercase();
        let mut all_nodes = Vec::new();
        self.collect_search_nodes(
            &self.data.clone(),
            Vec::new(),
            "root".to_string(),
            &mut all_nodes,
        );

        let matches: Vec<_> = all_nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.key.to_lowercase().contains(&query_lower)
                    || node.value.to_lowercase().contains(&query_lower)
            })
            .collect();

        self.search_total_matches = matches.len();

        if matches.is_empty() {
            self.search_current_match_index = 0;
            return;
        }

        let current_path = self
            .selected_node()
            .map(|n| n.path.clone())
            .unwrap_or_default();
        if let Some(pos) = matches
            .iter()
            .position(|(_, node)| node.path == current_path)
        {
            self.search_current_match_index = pos + 1;
        } else {
            self.search_current_match_index = 0;
        }
    }

    pub fn perform_search_realtime(&mut self, query: &str) {
        if query.is_empty() {
            self.update_search_match_stats(query);
            return;
        }

        let query_lower = query.to_lowercase();

        // Check if currently selected node already matches
        if let Some(node) = self.selected_node() {
            let is_matched_key = node.key.to_lowercase().contains(&query_lower);
            let is_matched_value = match node.value_type {
                ValueType::Object | ValueType::Array => false,
                _ => node.value_display.to_lowercase().contains(&query_lower),
            };
            if is_matched_key || is_matched_value {
                self.update_search_match_stats(query);
                return;
            }
        }

        let mut all_nodes = Vec::new();
        self.collect_search_nodes(
            &self.data.clone(),
            Vec::new(),
            "root".to_string(),
            &mut all_nodes,
        );

        // Find matches
        let matches: Vec<_> = all_nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.key.to_lowercase().contains(&query_lower)
                    || node.value.to_lowercase().contains(&query_lower)
            })
            .collect();

        if matches.is_empty() {
            self.update_search_match_stats(query);
            return;
        }

        // In real-time mode, we jump to the first match
        let target = matches.first().map(|(_, node)| node).unwrap();

        // Expand all ancestors
        let mut ancestors = Vec::new();
        for i in 1..target.path.len() {
            ancestors.push(target.path[..i].to_vec());
        }

        for p in ancestors {
            if let Some(node) = self.flattened_nodes.iter_mut().find(|n| n.path == p) {
                node.expanded = true;
            } else {
                // Force expansion of previously unflattened node
                self.flattened_nodes.push(UiNode {
                    path: p,
                    depth: 0,
                    key: String::new(),
                    value_display: String::new(),
                    value_type: ValueType::Null,
                    node_type: NodeType::Leaf,
                    expanded: true,
                });
            }
        }

        self.rebuild_flattened();

        // Find new index
        if let Some(pos) = self
            .flattened_nodes
            .iter()
            .position(|n| n.path == target.path)
        {
            self.selected = pos;
            // Center viewport if needed
            if self.selected < self.scroll_offset
                || self.selected >= self.scroll_offset + self.viewport_height
            {
                self.scroll_offset = self.selected.saturating_sub(self.viewport_height / 2);
            }
        }

        self.update_search_match_stats(query);
    }

    fn collect_search_nodes(
        &self,
        value: &Value,
        path: Vec<String>,
        key: String,
        nodes: &mut Vec<SearchNode>,
    ) {
        let value_display = match value {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format!("\"{}\"", s),
            Value::Array(_) | Value::Object(_) => String::new(),
        };

        nodes.push(SearchNode {
            path: path.clone(),
            key,
            value: value_display,
        });

        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let mut child_path = path.clone();
                    child_path.push(k.clone());
                    self.collect_search_nodes(v, child_path, k.clone(), nodes);
                }
            }
            Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(i.to_string());
                    self.collect_search_nodes(v, child_path, i.to_string(), nodes);
                }
            }
            _ => {}
        }
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn selected_node(&self) -> Option<&UiNode> {
        self.flattened_nodes.get(self.selected)
    }

    /// Get completion suggestions based on the current cursor node.
    pub fn get_completions_at_cursor(&self) -> Vec<CompletionItem> {
        if let Some(node) = self.selected_node() {
            self.get_completions_for_path(&node.path)
        } else {
            Vec::new()
        }
    }

    /// Get completion suggestions at a specific path.
    pub fn get_completions_for_path(&self, path: &[String]) -> Vec<CompletionItem> {
        crate::edit::get_completions_for_path(self, path)
    }

    /// Apply a completion item to a specific path.
    pub fn apply_completion(&mut self, path: &[String], item: &CompletionItem) {
        crate::edit::apply_completion(self, path, item);
    }

    /// Automatically determine node expansions from the top level based on screen height (limit: height * 2)
    pub fn auto_adjust_expansion(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let limit = height * 2;

        // 1. Calculate the number of nodes per level
        let mut counts = Vec::new();
        crate::flatten::count_nodes_per_level(&self.data, 0, &mut counts);

        // 2. Calculate the maximum depth that can be expanded within the limit
        let mut max_expand_depth = 0;
        let mut current_total = counts[0];

        for d in 0..counts.len() {
            if d + 1 < counts.len() {
                if current_total + counts[d + 1] <= limit {
                    current_total += counts[d + 1];
                    max_expand_depth = d;
                } else {
                    break;
                }
            } else {
                max_expand_depth = d;
                break;
            }
        }

        let mut dummy_prev_nodes = Vec::new();
        self.collect_expansion_paths(
            &self.data,
            Vec::new(),
            0,
            max_expand_depth,
            &mut dummy_prev_nodes,
        );

        // 4. Rebuild the flattened view based on the dummy node list
        self.flattened_nodes = crate::flatten::rebuild_flattened(
            &self.data,
            &dummy_prev_nodes,
            self.show_child_counts,
            self.schema.as_ref(),
        );
    }

    fn collect_expansion_paths(
        &self,
        value: &serde_json::Value,
        path: Vec<String>,
        depth: usize,
        max_depth: usize,
        nodes: &mut Vec<UiNode>,
    ) {
        if depth > max_depth {
            return;
        }

        let is_container = matches!(
            value,
            serde_json::Value::Object(_) | serde_json::Value::Array(_)
        );
        if !is_container {
            return;
        }

        nodes.push(UiNode {
            path: path.clone(),
            depth,
            key: String::new(),
            value_display: String::new(),
            value_type: ValueType::Null, // Dummy
            node_type: NodeType::Leaf,   // Dummy
            expanded: true,
        });

        match value {
            serde_json::Value::Object(map) => {
                let keys: Vec<_> = map.keys().collect();
                for k in keys {
                    let mut child_path = path.clone();
                    child_path.push(k.clone());
                    self.collect_expansion_paths(&map[k], child_path, depth + 1, max_depth, nodes);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(i.to_string());
                    self.collect_expansion_paths(v, child_path, depth + 1, max_depth, nodes);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
        s.char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or_else(|| s.len())
    }

    pub fn handle_key_event(&mut self, event: crossterm::event::KeyEvent) -> crate::action::Action {
        let prev_selected = self.selected;
        let action = self.handle_key_event_inner(event);
        if self.selected != prev_selected || !matches!(self.edit_mode, EditMode::Normal) {
            self.scroll_to_selected = true;
        }
        action
    }

    fn handle_key_event_inner(
        &mut self,
        event: crossterm::event::KeyEvent,
    ) -> crate::action::Action {
        use crate::action::Action;
        use crossterm::event::{KeyCode, KeyModifiers};

        // Global keys: Ctrl+C should always quit
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }

        // Reset blink timer on relevant keys
        match event.code {
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Enter => {
                self.last_cursor_activity = std::time::Instant::now();
            }
            _ => {}
        }

        let is_backspace_repeat = if event.code == KeyCode::Backspace {
            let now = self.last_cursor_activity;
            let repeat = if let Some(last) = self.last_backspace_time {
                now.duration_since(last).as_millis() < 180
            } else {
                false
            };
            self.last_backspace_time = Some(now);
            repeat
        } else {
            self.last_backspace_time = None;
            false
        };

        match &mut self.edit_mode {
            EditMode::Normal => {
                // Handle 's' for saving
                if event.code == KeyCode::Char('s')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    return Action::Save {
                        data: self.data.clone(),
                        format: self.format,
                    };
                }

                // Handle 'u' for Undo
                if event.code == KeyCode::Char('u')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    self.undo();
                    return Action::Noop;
                }

                // Handle 'r' for Redo
                if event.code == KeyCode::Char('r')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    self.redo();
                    return Action::Noop;
                }

                // Handle T for toggling type hints
                if event.code == KeyCode::Char('t')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    self.show_type_hints = !self.show_type_hints;
                    return Action::Noop;
                }

                // Handle K for toggling child counts
                if event.code == KeyCode::Char('k')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    self.show_child_counts = !self.show_child_counts;
                    self.rebuild_flattened();
                    return Action::Noop;
                }

                // Handle '/' for search
                if event.code == KeyCode::Char('/')
                    && !event.modifiers.contains(KeyModifiers::CONTROL)
                    && !event.modifiers.contains(KeyModifiers::ALT)
                {
                    self.edit_mode = EditMode::SearchPrompt {
                        buffer: String::new(),
                        cursor_pos: 0,
                    };
                    self.search_query = Some(String::new());
                    return Action::Noop;
                }

                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    match event.code {
                        KeyCode::Up => {
                            self.move_node_up();
                            return Action::Noop;
                        }
                        KeyCode::Down => {
                            self.move_node_down();
                            return Action::Noop;
                        }
                        _ => {}
                    }
                }

                match event.code {
                    KeyCode::Char('?') => {
                        self.edit_mode = EditMode::Help;
                        return Action::Noop;
                    }
                    KeyCode::Up => crate::navigate::move_up(self),
                    KeyCode::Down => crate::navigate::move_down(self),
                    KeyCode::PageUp => crate::navigate::page_up(self),
                    KeyCode::PageDown => crate::navigate::page_down(self),
                    KeyCode::Left => crate::navigate::collapse_current(self),
                    KeyCode::Right => crate::navigate::expand_or_move_to_last_child(self),
                    KeyCode::Char(' ') => crate::navigate::toggle_expand(self),
                    KeyCode::Enter => {
                        if let Some(node) = self.selected_node().cloned() {
                            match node.node_type {
                                NodeType::Object { .. } | NodeType::Array { .. } => {
                                    crate::edit::trigger_add_child(self);
                                }
                                NodeType::Leaf => {
                                    crate::edit::start_edit(self);
                                }
                            }
                        } else {
                            crate::edit::start_edit(self);
                        }
                    }
                    KeyCode::Backspace => {
                        crate::edit::start_edit_cleared(self);
                    }
                    KeyCode::Esc | KeyCode::Char('q')
                        if event.code == KeyCode::Esc
                            || (!event.modifiers.contains(KeyModifiers::CONTROL)
                                && !event.modifiers.contains(KeyModifiers::ALT)) =>
                    {
                        if self.search_query.is_some() {
                            self.search_query = None;
                            self.search_total_matches = 0;
                            self.search_current_match_index = 0;
                            return Action::Noop;
                        }
                        let is_actually_dirty = self.is_dirty
                            && (self.data != self.original_data || self.key_order_changed);
                        if is_actually_dirty {
                            self.edit_mode = EditMode::SavePrompt { selected: 0 };
                            return Action::Noop;
                        } else {
                            return Action::Quit;
                        }
                    }
                    KeyCode::Delete | KeyCode::Char('d')
                        if event.code == KeyCode::Delete
                            || (!event.modifiers.contains(KeyModifiers::CONTROL)
                                && !event.modifiers.contains(KeyModifiers::ALT)) =>
                    {
                        if let Some(node) = self.selected_node() {
                            let path = node.path.clone();
                            let _ = self.delete_node(&path);
                        }
                    }
                    _ => {}
                }
            }
            EditMode::Help => {
                self.edit_mode = EditMode::Normal;
                return Action::Noop;
            }
            EditMode::SearchPrompt { buffer, cursor_pos } => {
                let mut run_realtime = false;
                let mut next_match = false;

                match event.code {
                    KeyCode::Enter => {
                        next_match = true;
                    }
                    KeyCode::Esc => {
                        if buffer.is_empty() || self.search_total_matches == 0 {
                            self.search_query = None;
                            self.search_total_matches = 0;
                            self.search_current_match_index = 0;
                        }
                        self.edit_mode = EditMode::Normal;
                        return Action::Noop;
                    }
                    KeyCode::Char(c) => {
                        let byte_idx = Self::char_to_byte_index(buffer, *cursor_pos);
                        buffer.insert(byte_idx, c);
                        *cursor_pos += 1;
                        self.search_query = Some(buffer.clone());
                        run_realtime = true;
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            let byte_idx = Self::char_to_byte_index(buffer, *cursor_pos);
                            buffer.remove(byte_idx);
                        }
                        self.search_query = Some(buffer.clone());
                        run_realtime = true;
                    }
                    KeyCode::Left => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right if *cursor_pos < buffer.chars().count() => {
                        *cursor_pos += 1;
                    }
                    _ => {}
                }

                if next_match {
                    let query = buffer.clone();
                    self.perform_search(&query);
                } else if run_realtime {
                    let query = if let EditMode::SearchPrompt { buffer, .. } = &self.edit_mode {
                        buffer.clone()
                    } else {
                        String::new()
                    };
                    self.perform_search_realtime(&query);
                }
                return Action::Noop;
            }
            EditMode::SavePrompt { selected } => match event.code {
                KeyCode::Left | KeyCode::Right => {
                    *selected = 1 - *selected;
                }
                KeyCode::Enter => {
                    if *selected == 0 {
                        return Action::Quit;
                    } else {
                        return Action::SaveAndQuit {
                            data: self.data.clone(),
                            format: self.format,
                        };
                    }
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    return Action::SaveAndQuit {
                        data: self.data.clone(),
                        format: self.format,
                    };
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    return Action::Quit;
                }
                KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                    self.edit_mode = EditMode::Normal;
                    return Action::Noop;
                }
                _ => {}
            },
            EditMode::TextPrompt { buffer, cursor_pos }
            | EditMode::NewKeyPrompt {
                buffer, cursor_pos, ..
            }
            | EditMode::RenameKeyPrompt {
                buffer, cursor_pos, ..
            } => {
                match event.code {
                    KeyCode::Enter => crate::edit::apply_edit(self),
                    KeyCode::Esc => crate::edit::cancel_edit(self),
                    KeyCode::Char(c) => {
                        let byte_idx = Self::char_to_byte_index(buffer, *cursor_pos);
                        buffer.insert(byte_idx, c);
                        *cursor_pos += 1;
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            let byte_idx = Self::char_to_byte_index(buffer, *cursor_pos);
                            buffer.remove(byte_idx);
                        } else if matches!(self.edit_mode, EditMode::TextPrompt { .. }) {
                            if is_backspace_repeat {
                                return Action::Noop;
                            }
                            // Transition from TextPrompt to RenameKeyPrompt when buffer is empty
                            if let Some(node) = self.selected_node().cloned().filter(|n| !n.path.is_empty()) {
                                    let mut parent_path = node.path.clone();
                                    let original_key = parent_path.pop().unwrap();

                                    // Check if parent is an object
                                    let is_parent_object = if parent_path.is_empty() {
                                        self.data.is_object()
                                    } else {
                                        self.data
                                            .pointer(&crate::state::to_json_pointer(&parent_path))
                                            .map(|v| v.is_object())
                                            .unwrap_or(false)
                                    };

                                    if is_parent_object {
                                        let current_value = self
                                            .data
                                            .pointer(&crate::state::to_json_pointer(&node.path))
                                            .cloned()
                                            .unwrap_or(serde_json::Value::Null);

                                        let mut new_buffer = original_key.clone();
                                        if !new_buffer.is_empty() {
                                            new_buffer.pop();
                                        }
                                        let new_cursor_pos = new_buffer.chars().count();

                                        self.edit_mode = EditMode::RenameKeyPrompt {
                                            parent_path,
                                            original_key,
                                            buffer: new_buffer,
                                            cursor_pos: new_cursor_pos,
                                            value: current_value,
                                        };
                                    }
                            }
                        }
                    }
                    KeyCode::Left => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right if *cursor_pos < buffer.chars().count() => {
                        *cursor_pos += 1;
                    }
                    _ => {}
                }
            }
            EditMode::Dropdown { options, selected }
            | EditMode::NewKeyDropdown {
                options, selected, ..
            } => match event.code {
                KeyCode::Enter => crate::edit::apply_edit(self),
                KeyCode::Esc => crate::edit::cancel_edit(self),
                KeyCode::Up if *selected > 0 => {
                    *selected -= 1;
                }
                KeyCode::Down if *selected + 1 < options.len() => {
                    *selected += 1;
                }
                _ => {}
            },
        }
        Action::Noop
    }
}

pub(crate) fn to_json_pointer(path: &[String]) -> String {
    if path.is_empty() {
        return "".to_string();
    }
    let mut s = String::new();
    for p in path {
        s.push('/');
        s.push_str(&p.replace('~', "~0").replace('/', "~1"));
    }
    s
}

pub(crate) fn find_changed_paths(
    v1: &Value,
    v2: &Value,
    current_path: Vec<String>,
    changed: &mut Vec<Vec<String>>,
) {
    if v1 != v2 {
        changed.push(current_path.clone());
        match (v1, v2) {
            (Value::Object(map1), Value::Object(map2)) => {
                let keys: std::collections::HashSet<_> = map1.keys().chain(map2.keys()).collect();
                for k in keys {
                    let mut next_path = current_path.clone();
                    next_path.push(k.clone());
                    let default_val = Value::Null;
                    let val1 = map1.get(k).unwrap_or(&default_val);
                    let val2 = map2.get(k).unwrap_or(&default_val);
                    find_changed_paths(val1, val2, next_path, changed);
                }
            }
            (Value::Array(arr1), Value::Array(arr2)) => {
                let max_len = arr1.len().max(arr2.len());
                for i in 0..max_len {
                    let mut next_path = current_path.clone();
                    next_path.push(i.to_string());
                    let default_val = Value::Null;
                    let val1 = arr1.get(i).unwrap_or(&default_val);
                    let val2 = arr2.get(i).unwrap_or(&default_val);
                    find_changed_paths(val1, val2, next_path, changed);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::json;

    #[test]
    fn test_auto_adjust_expansion() {
        let data = json!({
            "a": {
                "b": {
                    "c": 1
                }
            }
        });
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Initial state: root expanded, nested collapsed
        assert_eq!(state.flattened_nodes.len(), 2);

        state.auto_adjust_expansion(1);
        assert_eq!(state.flattened_nodes.len(), 2);

        state.auto_adjust_expansion(2);
        assert_eq!(state.flattened_nodes.len(), 4);
    }

    #[test]
    fn test_handle_key_event_toggle_type_hint() {
        let data = json!({});
        let mut state = EditorState::new(data, Format::Json, None, None);
        assert!(!state.show_type_hints);

        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let event = KeyEvent {
            code: KeyCode::Char('t'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        state.handle_key_event(event);
        assert!(state.show_type_hints);
    }

    #[test]
    fn test_handle_key_event_navigation_action() {
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 0;

        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let event = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        state.handle_key_event(event);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_handle_key_event_delete_node() {
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1; // "a"

        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let event = KeyEvent {
            code: KeyCode::Delete,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        state.handle_key_event(event);
        assert_eq!(state.data, json!({"b": 2}));
    }

    #[test]
    fn test_handle_key_event_add_child_trigger() {
        let data = json!({});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 0; // root

        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let event = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        state.handle_key_event(event);
        // Should enter NewKeyPrompt since there is no schema
        match state.edit_mode {
            EditMode::NewKeyPrompt { .. } => {}
            _ => panic!("Expected NewKeyPrompt"),
        }
    }

    #[test]
    fn test_add_child_to_object() {
        let data = json!({});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state
            .add_child_node(&[], Some("new_key".to_string()), json!("value"))
            .unwrap();

        assert_eq!(state.data["new_key"], "value");
        // Root + 1 child
        assert_eq!(state.flattened_nodes.len(), 2);
    }

    #[test]
    fn test_add_child_to_array() {
        let data = json!([]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.add_child_node(&[], None, json!(1)).unwrap();
        state.add_child_node(&[], None, json!(2)).unwrap();

        assert_eq!(state.data, json!([1, 2]));
    }

    #[test]
    fn test_delete_node_from_object() {
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.delete_node(&["a".to_string()]).unwrap();

        assert_eq!(state.data, json!({"b": 2}));
        assert_eq!(state.flattened_nodes.len(), 2); // root + "b"
    }

    #[test]
    fn test_delete_node_from_array_and_shift() {
        let data = json!([1, 2, 3]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.delete_node(&["1".to_string()]).unwrap(); // delete '2'

        assert_eq!(state.data, json!([1, 3]));
        // Check flattened paths
        assert_eq!(state.flattened_nodes[1].path, vec!["0".to_string()]);
        assert_eq!(state.flattened_nodes[2].path, vec!["1".to_string()]);
    }

    #[test]
    fn test_delete_key_navigation_bounds() {
        let data = json!({"a": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1; // "a"

        state.delete_node(&["a".to_string()]).unwrap();
        assert_eq!(state.selected, 0); // Back to root
    }

    #[test]
    fn test_undo_redo_basic() {
        let data = json!({"a": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // 1. Save initial state
        state.save_to_undo();

        // 2. Modify data
        state.data = json!({"a": 2});
        state.selected = 10;

        // 3. Perform Undo
        state.undo();
        assert_eq!(state.data, json!({"a": 1}));
        assert_eq!(state.selected, 0);

        // 4. Perform Redo
        state.redo();
        assert_eq!(state.data, json!({"a": 2}));
        assert_eq!(state.selected, 10);
    }

    #[test]
    fn test_undo_redo_delete() {
        let mut state = EditorState::new(json!({"a": 1, "b": 2}), Format::Json, None, None);

        // 1. Delete "a" (save_to_undo is called internally in delete_node)
        state.delete_node(&["a".to_string()]).unwrap();
        assert_eq!(state.data, json!({"b": 2}));

        // 2. Perform Undo
        state.undo();
        assert_eq!(state.data, json!({"a": 1, "b": 2}));

        // 3. Perform Redo
        state.redo();
        assert_eq!(state.data, json!({"b": 2}));
    }

    #[test]
    fn test_undo_cancel_add_child() {
        let mut state = EditorState::new(json!({}), Format::Json, None, None);
        state.selected = 0; // Root

        // 1. Execute trigger_add_child (creates temp node and calls save_to_undo)
        crate::edit::trigger_add_child(&mut state);
        assert!(!state.undo_stack.is_empty());

        // 2. Execute cancel_edit (removes temp node and calls pop_undo)
        crate::edit::cancel_edit(&mut state);
        assert_eq!(state.undo_stack.len(), 0);
    }

    #[test]
    fn test_dirty_flag() {
        let mut state = EditorState::new(json!({"a": 1}), Format::Json, None, None);
        assert!(!state.is_dirty);

        // Simulate data change
        state.save_to_undo();
        assert!(state.is_dirty);

        // Verify that the dirty flag is cleared when the Undo stack becomes empty (cancel_edit scenario)
        state.pop_undo();
        assert!(!state.is_dirty);
    }

    #[test]
    fn test_dirty_flag_with_undo_redo() {
        let mut state = EditorState::new(json!({"a": 1}), Format::Json, None, None);
        assert!(!state.is_dirty);

        // 1. Change data
        state.save_to_undo();
        state.data = json!({"a": 2});
        assert!(state.is_dirty);

        // 2. Undo -> stack becomes empty, should be not dirty
        state.undo();
        assert!(
            !state.is_dirty,
            "Should not be dirty after undoing all changes"
        );

        // 3. Redo -> change reapplied, should be dirty again
        state.redo();
        assert!(state.is_dirty, "Should be dirty after redo");
    }

    #[test]
    fn test_save_prompt_keys() {
        use crate::action::Action;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut state = EditorState::new(json!({"a": 1}), Format::Json, None, None);

        // 1. Not dirty -> Esc should Quit
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Quit));

        // 2. Dirty -> Esc should enter SavePrompt mode
        state.save_to_undo();
        state.data = json!({"a": 2});
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Noop));
        assert!(matches!(
            state.edit_mode,
            EditMode::SavePrompt { selected: 0 }
        ));

        // 3. SavePrompt mode: 'c' should back to Normal
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Noop));
        assert!(matches!(state.edit_mode, EditMode::Normal));

        // 4. SavePrompt mode: 'n' should Quit
        state.edit_mode = EditMode::SavePrompt { selected: 0 }; // Now 0 is 'No'
        let event = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Quit));

        // 5. SavePrompt mode: 'y' should SaveAndQuit
        state.edit_mode = EditMode::SavePrompt { selected: 1 }; // Now 1 is 'Yes'
        let event = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::SaveAndQuit { .. }));

        // 6. SavePrompt mode: Arrow keys should toggle selection
        state.edit_mode = EditMode::SavePrompt { selected: 0 };
        let event = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        state.handle_key_event(event);
        assert!(matches!(
            state.edit_mode,
            EditMode::SavePrompt { selected: 1 }
        ));

        let event = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
        state.handle_key_event(event);
        assert!(matches!(
            state.edit_mode,
            EditMode::SavePrompt { selected: 0 }
        ));

        // 7. Enter on No (selected: 0)
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Quit));

        // 8. Enter on Yes (selected: 1)
        state.edit_mode = EditMode::SavePrompt { selected: 1 };
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::SaveAndQuit { .. }));
    }

    #[test]
    fn test_handle_key_event_help() {
        let mut state = EditorState::new(
            serde_json::json!({}),
            crate::format::Format::Json,
            None,
            None,
        );
        state.edit_mode = EditMode::Normal;

        // '?' should switch to Help mode
        state.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty()));
        assert!(matches!(state.edit_mode, EditMode::Help));

        // Any key should switch back to Normal mode
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(matches!(state.edit_mode, EditMode::Normal));
    }

    #[test]
    fn test_handle_key_event_toggle_child_counts() {
        let data = json!({"a": [1, 2, 3]});
        let mut state = EditorState::new(data, Format::Json, None, None);
        // Default should be true
        assert!(state.show_child_counts);

        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        let event = KeyEvent {
            code: KeyCode::Char('k'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        state.handle_key_event(event);
        assert!(!state.show_child_counts);

        state.handle_key_event(event);
        assert!(state.show_child_counts);
    }

    #[test]
    fn test_move_node_up_down_in_array() {
        let data = json!([1, 2, 3]);
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Root is 0, [1] is 1, [2] is 2, [3] is 3
        state.selected = 2; // Select "2"
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Move up: [2, 1, 3]
        state.move_node_up();
        assert_eq!(state.data, json!([2, 1, 3]));
        assert_eq!(state.selected, 1); // Follow the moved node
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Move down: [1, 2, 3]
        state.move_node_down();
        assert_eq!(state.data, json!([1, 2, 3]));
        assert_eq!(state.selected, 2);
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Move down again: [1, 3, 2]
        state.move_node_down();
        assert_eq!(state.data, json!([1, 3, 2]));
        assert_eq!(state.selected, 3);
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Undo
        state.undo();
        assert_eq!(state.data, json!([1, 2, 3]));
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn test_move_node_up_down_in_object() {
        let data = json!({"a": 1, "b": 2, "c": 3});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Root is 0, "a" is 1, "b" is 2, "c" is 3
        state.selected = 2; // Select "b"
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Move up: {"b": 2, "a": 1, "c": 3}
        state.move_node_up();
        let keys: Vec<_> = state.data.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["b", "a", "c"]);
        assert_eq!(state.selected, 1); // Follow "b"
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Move down: {"a": 1, "b": 2, "c": 3}
        state.move_node_down();
        let keys: Vec<_> = state.data.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(state.selected, 2);
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Move down again: {"a": 1, "c": 3, "b": 2}
        state.move_node_down();
        let keys: Vec<_> = state.data.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["a", "c", "b"]);
        assert_eq!(state.selected, 3);
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Undo
        state.undo();
        let keys: Vec<_> = state.data.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn test_handle_key_event_shortcuts() {
        use crate::action::Action;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = json!({"a": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // --- Save shortcut ---
        // New: 's' (No modifiers)
        let event = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Save { .. }));

        // Old: 'Ctrl+S' (Should not trigger Action::Save anymore)
        let event = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let action = state.handle_key_event(event);
        assert!(!matches!(action, Action::Save { .. }));

        // --- Undo shortcut ---
        state.save_to_undo();
        assert_eq!(state.undo_stack.len(), 1);

        // New: 'u' (No modifiers)
        let event = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE);
        state.handle_key_event(event);
        assert_eq!(state.undo_stack.len(), 0);
        assert_eq!(state.redo_stack.len(), 1);

        // Old: 'Ctrl+Z' (Should not trigger Undo)
        state.save_to_undo();
        assert_eq!(state.undo_stack.len(), 1);
        let event = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
        state.handle_key_event(event);
        assert_eq!(state.undo_stack.len(), 1);

        // --- Redo shortcut ---
        // Setup: state has 1 undo, 1 redo (from 'u' check).
        // Let's clear and setup specifically.
        state.undo_stack.clear();
        state.redo_stack.clear();
        state.save_to_undo(); // stack: [1]
        state.undo(); // stack: [], redo: [1]
        assert_eq!(state.redo_stack.len(), 1);

        // New: 'r' (No modifiers)
        let event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        state.handle_key_event(event);
        assert_eq!(state.redo_stack.len(), 0);
        assert_eq!(state.undo_stack.len(), 1);

        // Old: 'Ctrl+Y' (Should not trigger Redo)
        state.undo(); // stack: [], redo: [1]
        let event = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        state.handle_key_event(event);
        assert_eq!(state.redo_stack.len(), 1);
    }

    #[test]
    fn test_search_action_finds_and_expands() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = serde_json::json!({
            "a": {
                "b": {
                    "c": "target"
                }
            }
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Initial state: root is expanded, others collapsed by default?
        // Actually EditorState::new calls rebuild_flattened which uses dummy prev_nodes (empty)
        // Let's check initial expansion.
        assert_eq!(state.flattened_nodes.len(), 2); // root and "a"

        // Enter search mode
        let event = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        state.handle_key_event(event);
        assert!(matches!(state.edit_mode, EditMode::SearchPrompt { .. }));

        // Input "target"
        if let EditMode::SearchPrompt { buffer, cursor_pos } = &mut state.edit_mode {
            *buffer = "target".to_string();
            *cursor_pos = buffer.len();
        }

        // Press Enter to search
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        state.handle_key_event(event);

        // Should find "target", expand parents, and select it
        assert!(matches!(state.edit_mode, EditMode::SearchPrompt { .. }));

        // After expansion:
        // 0: root
        // 1: a
        // 2: b
        // 3: c
        assert_eq!(state.flattened_nodes.len(), 4);
        assert_eq!(state.selected, 3);
        assert_eq!(state.flattened_nodes[state.selected].key, "c");
        assert_eq!(
            state.flattened_nodes[state.selected].value_display,
            "\"target\""
        );

        // Escape to exit search mode
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::Normal));
    }

    #[test]
    fn test_search_query_updates_realtime() {
        let data = serde_json::json!({
            "key1": "value1",
            "key2": "value2"
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // 1. Enter search mode
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::SearchPrompt { .. }));
        assert_eq!(state.search_total_matches, 0);
        assert_eq!(state.search_current_match_index, 0);

        // 2. Type 'k' -> matches "key1" and "key2" (total 2)
        state.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("k".to_string()));
        assert_eq!(state.search_total_matches, 2);
        assert_eq!(state.search_current_match_index, 1);

        // 3. Type 'e' -> "ke", matches both (total 2)
        state.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("ke".to_string()));
        assert_eq!(state.search_total_matches, 2);
        assert_eq!(state.search_current_match_index, 1);

        // 4. Backspace -> "k"
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("k".to_string()));
        assert_eq!(state.search_total_matches, 2);
        assert_eq!(state.search_current_match_index, 1);

        // 5. Escape to exit prompt -> keeps stats
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("k".to_string()));
        assert_eq!(state.search_total_matches, 2);
        assert!(matches!(state.edit_mode, EditMode::Normal));

        // 5b. Second Escape in Normal mode -> clears stats
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.search_query, None);
        assert_eq!(state.search_total_matches, 0);

        // 6. Enter to confirm (keeps search active, goes to next match)
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));

        // First match is index 1 ("key1")
        assert_eq!(state.search_current_match_index, 1);

        // Enter -> moves to next match ("key2", index 2)
        state.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("k".to_string()));
        assert_eq!(state.search_total_matches, 2);
        assert_eq!(state.search_current_match_index, 2);
        assert!(matches!(state.edit_mode, EditMode::SearchPrompt { .. }));

        // 7. Escape to exit prompt -> keeps stats
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.search_query, Some("k".to_string()));
        assert!(matches!(state.edit_mode, EditMode::Normal));

        // 7b. Second Escape in Normal mode -> clears stats
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.search_query, None);
        assert_eq!(state.search_total_matches, 0);
        assert_eq!(state.search_current_match_index, 0);
    }

    #[test]
    fn test_perform_search_realtime_navigation() {
        let data = serde_json::json!({
            "group1": {
                "item_a": "target",
                "item_b": "other"
            },
            "group2": {
                "target_key": "value"
            }
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // 1. Search for "target"
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        // Type 't'
        state.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));

        // Check if it moved to group1.item_a (first match)
        let selected_node = state.selected_node().unwrap();
        assert!(selected_node.key.contains("t") || selected_node.value_display.contains("t"));

        // Type 'a' -> "ta"
        state.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));

        // Still should be at group1.item_a
        let selected_node = state.selected_node().unwrap();
        assert_eq!(selected_node.key, "item_a");

        // Search for "target_key" by typing more
        // Current query "ta". Let's type "rget_k"
        for c in "rget_k".chars() {
            state.handle_key_event(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        // Should move to group2.target_key
        let selected_node = state.selected_node().unwrap();
        assert_eq!(selected_node.key, "target_key");

        // Check if parent (group2) is expanded
        let group2_node = state
            .flattened_nodes
            .iter()
            .find(|n| n.key == "group2")
            .unwrap();
        assert!(group2_node.expanded);
    }

    #[test]
    fn test_search_esc_double_press_behavior() {
        let data = serde_json::json!({
            "key1": "value1",
            "key2": "value2"
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Scenario 1: SearchPrompt with results + ESC -> Normal mode, search_query remains
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.search_total_matches, 2);

        // Press ESC in SearchPrompt
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::Normal));
        assert_eq!(
            state.search_query,
            Some("k".to_string()),
            "Search query should be preserved when results exist"
        );
        assert_eq!(state.search_total_matches, 2);

        // Press ESC in Normal mode with search_query active -> Reset search
        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            state.search_query, None,
            "Search query should be cleared on second ESC"
        );
        assert_eq!(state.search_total_matches, 0);

        // Scenario 2: SearchPrompt with NO results + ESC -> Reset immediately
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(state.search_total_matches, 0);

        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::Normal));
        assert_eq!(
            state.search_query, None,
            "Search query should be cleared immediately if no matches"
        );

        // Scenario 3: SearchPrompt with EMPTY buffer + ESC -> Reset immediately
        state.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(state.search_total_matches, 0);

        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::Normal));
        assert_eq!(
            state.search_query, None,
            "Search query should be cleared immediately if buffer is empty"
        );
    }

    #[test]
    fn test_backspace_to_rename_key() {
        use crate::edit::apply_edit;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = serde_json::json!({"name": "val"});
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // 1. Enter edit mode on "name" value
        state.selected = 1; // "name" node
        state.edit_mode = EditMode::TextPrompt {
            buffer: "val".to_string(),
            cursor_pos: 3,
        };

        // 2. Clear buffer
        if let EditMode::TextPrompt { buffer, cursor_pos } = &mut state.edit_mode {
            *buffer = "".to_string();
            *cursor_pos = 0;
        }

        // 3. Press Backspace again -> Should transition to RenameKeyPrompt
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        match &state.edit_mode {
            EditMode::RenameKeyPrompt {
                original_key,
                buffer,
                ..
            } => {
                assert_eq!(original_key, "name");
                assert_eq!(buffer, "nam"); // "name" minus last char
            }
            _ => panic!("Expected RenameKeyPrompt, got {:?}", state.edit_mode),
        }

        // 4. Change key to "key" and apply
        if let EditMode::RenameKeyPrompt {
            buffer, cursor_pos, ..
        } = &mut state.edit_mode
        {
            *buffer = "key".to_string();
            *cursor_pos = 3;
        }
        apply_edit(&mut state);

        // 5. Verify result
        assert_eq!(state.data, serde_json::json!({"key": "val"}));
        assert_eq!(state.flattened_nodes[1].key, "key");
    }

    #[test]
    fn test_backspace_to_rename_key_for_object() {
        use crate::edit::apply_edit;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = serde_json::json!({"settings": {"theme": "dark"}});
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // 1. Select the "settings" node (which is an Object)
        state.selected = 1; // "settings" node
        assert_eq!(state.flattened_nodes[1].key, "settings");

        // 2. Press Backspace on the Object node directly -> Should transition to RenameKeyPrompt
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        match &state.edit_mode {
            EditMode::RenameKeyPrompt {
                original_key,
                buffer,
                value,
                ..
            } => {
                assert_eq!(original_key, "settings");
                assert_eq!(buffer, "setting"); // "settings" minus last char
                assert_eq!(value, &serde_json::json!({"theme": "dark"})); // Preserves the entire object
            }
            _ => panic!("Expected RenameKeyPrompt, got {:?}", state.edit_mode),
        }

        // 3. Change key to "config" and apply
        if let EditMode::RenameKeyPrompt {
            buffer, cursor_pos, ..
        } = &mut state.edit_mode
        {
            *buffer = "config".to_string();
            *cursor_pos = 6;
        }
        apply_edit(&mut state);

        // 4. Verify result
        assert_eq!(state.data, serde_json::json!({"config": {"theme": "dark"}}));
        assert_eq!(state.flattened_nodes[1].key, "config");
    }

    #[test]
    fn test_undo_redo_expands_collapsed_nodes() {
        let data = serde_json::json!({
            "nested": {
                "key": "value"
            }
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Initial state: "nested" is collapsed
        assert_eq!(state.flattened_nodes.len(), 2);
        assert!(!state.flattened_nodes[1].expanded);

        // 1. Save undo
        state.save_to_undo();

        // 2. Modify "nested/key"
        let key_pointer = to_json_pointer(&["nested".to_string(), "key".to_string()]);
        if let Some(val) = state.data.pointer_mut(&key_pointer) {
            *val = serde_json::json!("new_value");
        }
        state.rebuild_flattened();

        // 3. Perform Undo
        state.undo();

        // "nested" should be expanded now
        assert!(state.flattened_nodes[1].expanded);
        assert_eq!(state.flattened_nodes.len(), 3);
        assert_eq!(state.flattened_nodes[2].key, "key");

        // 4. Collapse again
        state.flattened_nodes[1].expanded = false;
        state.rebuild_flattened();
        assert_eq!(state.flattened_nodes.len(), 2);

        // 5. Perform Redo
        state.redo();
        assert!(state.flattened_nodes[1].expanded);
        assert_eq!(state.flattened_nodes.len(), 3);
    }

    #[test]
    fn test_backspace_safeguard() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = serde_json::json!({"name": "val"});
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // 1. Enter edit mode on "name" value with empty buffer
        state.selected = 1; // "name" node
        state.edit_mode = EditMode::TextPrompt {
            buffer: "".to_string(),
            cursor_pos: 0,
        };

        // 2. Press Backspace (first time)
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        // Let's refine the test:
        state.edit_mode = EditMode::TextPrompt {
            buffer: "v".to_string(),
            cursor_pos: 1,
        };

        // First backspace: "v" -> ""
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(
            matches!(state.edit_mode, EditMode::TextPrompt { ref buffer, .. } if buffer.is_empty())
        );

        // Second backspace (repeat): "" -> "" (stay in TextPrompt)
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::TextPrompt { .. }));

        // Wait > 180ms
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Third backspace (new press): "" -> RenameKeyPrompt
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::RenameKeyPrompt { .. }));
    }

    #[test]
    fn test_handle_key_event_q_quits_like_esc() {
        use crate::action::Action;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = json!({"a": 1});
        let mut state = EditorState::new(data.clone(), Format::Json, None, None);

        // 'q' without modifiers should return Action::Quit if not dirty
        let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let action = state.handle_key_event(event);
        assert!(matches!(action, Action::Quit));

        // 'Ctrl+q' should not trigger quit
        let mut state = EditorState::new(data, Format::Json, None, None);
        let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let action = state.handle_key_event(event);
        assert!(!matches!(action, Action::Quit));
    }

    #[test]
    fn test_handle_key_event_d_deletes_like_delete() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1; // "a" (index 0 is root {})

        // 'd' without modifiers should delete the selected node
        let event = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        state.handle_key_event(event);
        assert_eq!(state.data, json!({"b": 2}));

        // Reset and test 'Ctrl+d'
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1;
        let event = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        state.handle_key_event(event);
        assert_eq!(state.data, json!({"a": 1, "b": 2}));
    }
}
