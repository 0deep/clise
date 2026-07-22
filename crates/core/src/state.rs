use crate::format::Format;
use crate::node::{AnnotatedNode, find_node_by_path, nodes_to_active_value};
use crate::util::char_to_byte_index;
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
    /// Whether this node represents a commented-out code (DisabledCode)
    pub is_disabled_comment: bool,
    /// Whether this node has any comments (above, inline, or disabled)
    pub has_comment: bool,
    /// Preview text of the first comment (above or inline), stripped of markers
    pub comment_preview: Option<String>,
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
        descriptions: Vec<Option<String>>,
        selected: usize,
        scroll_offset: usize,
        filter_buffer: String,
        filtered_indices: Vec<usize>,
    },
    /// Text input prompt
    TextPrompt { buffer: String, cursor_pos: usize },
    /// Dropdown for adding a new key (includes parent node path and temporary key)
    NewKeyDropdown {
        parent_path: Vec<String>,
        temp_key: String,
        options: Vec<String>,
        descriptions: Vec<Option<String>>,
        selected: usize,
        scroll_offset: usize,
        filter_buffer: String,
        cursor_pos: usize,
        filtered_indices: Vec<usize>,
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
    Help {
        scroll_offset: usize,
        max_offset: usize,
    },
    /// Dropdown for selecting a oneOf/anyOf variant when value is null
    OneOfVariantDropdown {
        parent_path: Vec<String>,
        target_key: String,
        options: Vec<String>,
        descriptions: Vec<Option<String>>,
        selected: usize,
        scroll_offset: usize,
        filter_buffer: String,
        cursor_pos: usize,
        filtered_indices: Vec<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitResult {
    /// Clicked on ▶/▼ triangle (toggle collapse/expand)
    Triangle,
    /// Clicked on key name
    Key,
    /// Clicked on value
    Value,
    /// Empty area
    None,
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
    nodes: Vec<AnnotatedNode>,
    root: usize,
    selected: usize,
    key_order_changed: bool,
    renamed_keys: std::collections::HashMap<String, String>,
}

/// Full state of the editor
pub struct EditorState {
    /// Root node index into `nodes` (usually 0).
    pub root: usize,
    /// Integrated node vec (active + disabled + comments).
    pub nodes: Vec<AnnotatedNode>,
    /// Snapshot for dirty detection (compared to `nodes`).
    pub original_nodes: Vec<AnnotatedNode>,
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
    /// Flattened index of the node hovered by mouse (None if no hover)
    pub hovered_node: Option<usize>,
    /// Whether to scroll viewport to keep selected node visible
    pub scroll_to_selected: bool,
    /// Number of visible items in the current dropdown popup
    pub dropdown_visible_items: usize,
    /// Cache of all encountered nodes' expanded states to preserve them even when collapsed
    pub(crate) all_nodes_cache: Vec<UiNode>,
    /// Last time backspace was pressed (to prevent accidental rename prompt)
    pub last_backspace_time: Option<std::time::Instant>,
    /// Tooltip state (scroll offset, area, max width)
    pub tooltip: crate::tooltip::TooltipState,
    /// Area of the currently rendered dropdown menu (if active)
    pub dropdown_area: Option<ratatui::layout::Rect>,
}

impl EditorState {
    pub fn new(
        data: Value,
        format: Format,
        filename: Option<String>,
        original_text: Option<String>,
    ) -> Self {
        // Compose bridge: original_text Some → parse_annotated; None → value_to_annotated.
        let (nodes, root) = if let Some(text) = original_text.as_deref() {
            match crate::format::parse_annotated(text, format) {
                Ok(r) => r,
                Err(_) => crate::format::value_to_annotated(&data),
            }
        } else {
            crate::format::value_to_annotated(&data)
        };

        let original_nodes = nodes.clone();

        let mut state = Self {
            root,
            nodes,
            original_nodes,
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
            key_order_changed: false,
            last_cursor_activity: std::time::Instant::now(),
            search_query: None,
            search_total_matches: 0,
            search_current_match_index: 0,
            renamed_keys: std::collections::HashMap::new(),
            hovered_node: None,
            scroll_to_selected: true,
            dropdown_visible_items: 10,
            all_nodes_cache: Vec::new(),
            last_backspace_time: None,
            tooltip: crate::tooltip::TooltipState::new(),
            dropdown_area: None,
        };
        state.rebuild_flattened();
        state
    }

    /// Look up an AnnotatedNode by JSON path. Returns None if path not found.
    pub fn node_at_path(&self, path: &[String]) -> Option<&AnnotatedNode> {
        find_node_by_path(&self.nodes, path).map(|idx| &self.nodes[idx])
    }

    /// Mutable lookup by path.
    pub fn node_at_path_mut(&mut self, path: &[String]) -> Option<&mut AnnotatedNode> {
        find_node_by_path(&self.nodes, path).and_then(move |idx| self.nodes.get_mut(idx))
    }

    /// Thin bridge for legacy `self.data.pointer(&ptr)` callers. Returns the
    /// node's `value` regardless of `is_active`.
    pub fn node_at_path_as_value(&self, path: &[String]) -> Option<&Value> {
        self.node_at_path(path).map(|n| &n.value)
    }

    /// Mutable value bridge.
    pub fn node_at_path_mut_value(&mut self, path: &[String]) -> Option<&mut Value> {
        self.node_at_path_mut(path).map(|n| &mut n.value)
    }

    /// Reconstruct the active-only Value tree (for jsonschema validation etc).
    pub fn active_value(&self) -> Value {
        nodes_to_active_value(&self.nodes, self.root)
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

    fn calculate_type_hint_width(&self, node: &UiNode) -> u16 {
        if !self.show_type_hints {
            return 0;
        }
        let Some(schema) = &self.schema else { return 0 };
        let Some(sub) = crate::schema_util::find_sub_schema(schema, &node.path) else {
            return 0;
        };
        let type_hint_text = crate::schema_util::extract_type_hint_for_value(
            sub,
            self.node_at_path_as_value(&node.path),
        );
        unicode_width::UnicodeWidthStr::width(type_hint_text.as_str()) as u16
    }

    /// Given a y offset relative to the list area top, return the flattened node index.
    pub fn node_index_at_y(&self, y: usize, list_area: ratatui::layout::Rect) -> Option<usize> {
        let mut current_y: usize = 0;
        let mut idx = self.scroll_offset;
        while idx < self.flattened_nodes.len() {
            let mut lines = 1;
            if idx == self.selected {
                if let Some(node) = self.selected_node() {
                    let x_offset = (node.depth as u16).saturating_mul(2);
                    let actual_key_len =
                        unicode_width::UnicodeWidthStr::width(node.key.as_str()) as u16;
                    let type_hint_width = self.calculate_type_hint_width(node);
                    let colon_x = list_area
                        .x
                        .saturating_add(x_offset)
                        .saturating_add(2)
                        .saturating_add(actual_key_len)
                        .saturating_add(type_hint_width);
                    let first_line_val_x = colon_x.saturating_add(2);
                    let first_line_width =
                        list_area.right().saturating_sub(first_line_val_x) as usize;
                    let wrapped_val_x = list_area.x.saturating_add(x_offset).saturating_add(2);
                    let wrapped_line_width =
                        list_area.right().saturating_sub(wrapped_val_x) as usize;

                    let mut node_height = 1;
                    let text_to_measure = match &self.edit_mode {
                        EditMode::TextPrompt { buffer, .. }
                        | EditMode::NewKeyPrompt { buffer, .. } => Some(buffer.as_str()),
                        EditMode::Normal => Some(node.value_display.as_str()),
                        _ => None,
                    };
                    if let Some(text) = text_to_measure {
                        if text.len() > first_line_width && first_line_width > 0 {
                            let remaining = text.len() - first_line_width;
                            if wrapped_line_width > 0 {
                                node_height =
                                    1 + (remaining + wrapped_line_width - 1) / wrapped_line_width;
                            }
                        }
                    }

                    if node.depth > 0 {
                        if let Some(schema) = &self.schema {
                            if let Some(sub) =
                                crate::schema_util::find_sub_schema(schema, &node.path)
                            {
                                if let Some(desc) = crate::schema_util::extract_description(sub) {
                                    let node_x = list_area.x.saturating_add(x_offset);
                                    let max_tip_width = list_area
                                        .right()
                                        .saturating_sub(node_x)
                                        .saturating_sub(2)
                                        .clamp(20, 60);
                                    let tip_lines = crate::tooltip::count_markdown_lines(
                                        &desc,
                                        max_tip_width as usize,
                                    );
                                    let display_lines = tip_lines.min(8);
                                    node_height += display_lines + 2;
                                }
                            }
                        }
                    }

                    lines = node_height;
                }
            }

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

    /// Returns which UI element is located at the given coordinates.
    /// list_area: The Rect passed to render_list.
    pub fn hit_test(
        &self,
        x: u16,
        y: u16,
        list_area: ratatui::layout::Rect,
    ) -> (Option<usize>, HitResult) {
        if let Some(drop_area) = self.dropdown_area {
            if x >= drop_area.x
                && x < drop_area.x.saturating_add(drop_area.width)
                && y >= drop_area.y
                && y < drop_area.y.saturating_add(drop_area.height)
            {
                return (None, HitResult::None);
            }
        }

        if let Some(tip_area) = self.tooltip.area {
            if x >= tip_area.x
                && x < tip_area.x.saturating_add(tip_area.width)
                && y >= tip_area.y
                && y < tip_area.y.saturating_add(tip_area.height)
            {
                return (None, HitResult::None);
            }
        }

        // 1. Convert y coordinate to node index (using node_index_at_y)
        let node_idx =
            match self.node_index_at_y((y.saturating_sub(list_area.y)) as usize, list_area) {
                Some(idx) => idx,
                None => return (None, HitResult::None),
            };

        let node = match self.flattened_nodes.get(node_idx) {
            Some(n) => n,
            None => return (None, HitResult::None),
        };

        let x_offset = (node.depth as u16).saturating_mul(2);
        let prefix_x = list_area.x.saturating_add(x_offset); // Start of ▶/▼
        let key_x = prefix_x.saturating_add(2); // Start of key

        // Calculate key width
        let is_editing_key = match &self.edit_mode {
            EditMode::NewKeyPrompt {
                parent_path,
                temp_key,
                ..
            }
            | EditMode::NewKeyDropdown {
                parent_path,
                temp_key,
                ..
            } => node.path.starts_with(parent_path) && node.path.last() == Some(temp_key),
            EditMode::RenameKeyPrompt {
                parent_path,
                original_key,
                ..
            } => node.path.starts_with(parent_path) && node.path.last() == Some(original_key),
            _ => false,
        };

        let key_width = if is_editing_key {
            match &self.edit_mode {
                EditMode::NewKeyPrompt { buffer, .. }
                | EditMode::RenameKeyPrompt { buffer, .. } => {
                    unicode_width::UnicodeWidthStr::width(buffer.as_str()) as u16
                }
                EditMode::NewKeyDropdown { .. } => 12, // "(Select Key)" length
                _ => unicode_width::UnicodeWidthStr::width(node.key.as_str()) as u16,
            }
        } else {
            unicode_width::UnicodeWidthStr::width(node.key.as_str()) as u16
        };

        // Calculate type hint width
        let mut type_hint_width = 0;
        if !is_editing_key {
            type_hint_width = self.calculate_type_hint_width(node);
        }

        let colon_x = key_x
            .saturating_add(key_width)
            .saturating_add(type_hint_width);
        let value_x = colon_x.saturating_add(2); // After ": "

        // 2. Determine the region by x coordinate
        if x >= prefix_x && x < prefix_x.saturating_add(2) {
            // Triangle region
            match node.node_type {
                NodeType::Object { .. } | NodeType::Array { .. } => {
                    (Some(node_idx), HitResult::Triangle)
                }
                _ => (Some(node_idx), HitResult::Key),
            }
        } else if x >= key_x && x < colon_x {
            (Some(node_idx), HitResult::Key)
        } else if x >= value_x {
            (Some(node_idx), HitResult::Value)
        } else {
            (Some(node_idx), HitResult::Key)
        }
    }

    /// Select a dropdown item by its visible (filtered) index and commit the choice.
    /// Returns true if the item was selected and committed.
    pub fn select_dropdown_item(&mut self, visible_index: usize) -> bool {
        match &mut self.edit_mode {
            EditMode::Dropdown {
                selected,
                filtered_indices,
                ..
            } => {
                if visible_index < filtered_indices.len() {
                    let original_idx = filtered_indices[visible_index];
                    *selected = original_idx;
                    crate::edit::apply_edit(self);
                    return true;
                }
            }
            EditMode::NewKeyDropdown {
                selected,
                filtered_indices,
                ..
            } => {
                if visible_index < filtered_indices.len() {
                    let original_idx = filtered_indices[visible_index];
                    *selected = original_idx;
                    crate::edit::apply_edit(self);
                    return true;
                }
            }
            EditMode::OneOfVariantDropdown {
                selected,
                filtered_indices,
                ..
            } => {
                if visible_index < filtered_indices.len() {
                    let original_idx = filtered_indices[visible_index];
                    *selected = original_idx;
                    crate::edit::apply_oneof_variant(self);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Hover over a dropdown item by its visible (filtered) index.
    /// Moves the highlight without committing. Returns true if the hover was applied.
    pub fn hover_dropdown_item(&mut self, visible_index: usize) -> bool {
        match &mut self.edit_mode {
            EditMode::Dropdown {
                selected,
                filtered_indices,
                ..
            }
            | EditMode::NewKeyDropdown {
                selected,
                filtered_indices,
                ..
            }
            | EditMode::OneOfVariantDropdown {
                selected,
                filtered_indices,
                ..
            } => {
                if visible_index < filtered_indices.len() {
                    self.tooltip.reset_scroll();
                    *selected = visible_index;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// Scroll the dropdown list by delta items. Positive scrolls down, negative scrolls up.
    /// Returns true if the scroll was applied.
    pub fn scroll_dropdown(&mut self, delta: i32) -> bool {
        match &mut self.edit_mode {
            EditMode::Dropdown {
                selected,
                scroll_offset,
                filtered_indices,
                ..
            }
            | EditMode::NewKeyDropdown {
                selected,
                scroll_offset,
                filtered_indices,
                ..
            }
            | EditMode::OneOfVariantDropdown {
                selected,
                scroll_offset,
                filtered_indices,
                ..
            } => {
                let total = filtered_indices.len();
                if total == 0 {
                    return false;
                }
                self.tooltip.reset_scroll();
                let visible = self.dropdown_visible_items;
                if delta > 0 {
                    *selected = (*selected + delta as usize).min(total - 1);
                } else {
                    *selected = (*selected).saturating_sub((-delta) as usize);
                }
                if visible > 0 && *selected >= *scroll_offset + visible {
                    *scroll_offset = *selected + 1 - visible;
                }
                if *selected < *scroll_offset {
                    *scroll_offset = *selected;
                }
                true
            }
            _ => false,
        }
    }

    pub fn is_node_modified(&self, path: &[String]) -> bool {
        let curr = match self.node_at_path(path) {
            Some(n) => n,
            None => return false,
        };
        let orig = match find_node_by_path(&self.original_nodes, path) {
            Some(i) => &self.original_nodes[i],
            None => return true,
        };
        if curr.is_active != orig.is_active {
            return true;
        }
        match (&curr.value, &orig.value) {
            (Value::Object(c), Value::Object(o)) => {
                let ck: Vec<_> = c.keys().collect();
                let ok: Vec<_> = o.keys().collect();
                ck != ok
            }
            (Value::Array(c), Value::Array(o)) => c.len() != o.len(),
            (c, o) => c != o,
        }
    }

    pub fn on_save(&mut self) {
        self.original_nodes = self.nodes.clone();
        self.is_dirty = false;
        self.key_order_changed = false;
    }

    pub fn set_status(&mut self, message: String) {
        self.status_message = Some((message, std::time::Instant::now()));
    }

    /// Save the current state to the Undo stack
    pub fn save_to_undo(&mut self) {
        self.undo_stack.push(HistoryEntry {
            nodes: self.nodes.clone(),
            root: self.root,
            selected: self.selected,
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

    /// Pop the top entry of the Redo stack
    pub fn pop_redo(&mut self) {
        self.redo_stack.pop();
    }

    /// Undo to the previous state
    pub fn undo(&mut self) {
        if let Some(entry) = self.undo_stack.pop() {
            // Save the current state to the Redo stack
            self.redo_stack.push(HistoryEntry {
                nodes: self.nodes.clone(),
                root: self.root,
                selected: self.selected,
                key_order_changed: self.key_order_changed,
                renamed_keys: self.renamed_keys.clone(),
            });

            let old_nodes = self.nodes.clone();

            // Restore state
            self.nodes = entry.nodes;
            self.root = entry.root;
            self.selected = entry.selected;
            self.key_order_changed = entry.key_order_changed;
            self.renamed_keys = entry.renamed_keys;
            self.rebuild_flattened_impl(Some(&old_nodes));
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
                nodes: self.nodes.clone(),
                root: self.root,
                selected: self.selected,
                key_order_changed: self.key_order_changed,
                renamed_keys: self.renamed_keys.clone(),
            });

            let old_nodes = self.nodes.clone();

            // Restore state
            self.nodes = entry.nodes;
            self.root = entry.root;
            self.selected = entry.selected;
            self.key_order_changed = entry.key_order_changed;
            self.renamed_keys = entry.renamed_keys;
            self.rebuild_flattened_impl(Some(&old_nodes));
            self.set_status("Redo".to_string());
            self.is_dirty = true;
        }
    }

    fn sync_value_from_active_children(&mut self, idx: usize) {
        let rebuilt = match &self.nodes[idx].value {
            Value::Object(_) => {
                let mut map = serde_json::Map::new();
                for &ci in &self.nodes[idx].children {
                    let c = match self.nodes.get(ci) {
                        Some(c) => c,
                        None => continue,
                    };
                    if c.is_active {
                        if let Some(k) = c.path.last() {
                            map.insert(k.clone(), c.value.clone());
                        }
                    }
                }
                Value::Object(map)
            }
            Value::Array(_) => {
                let mut arr = Vec::new();
                for &ci in &self.nodes[idx].children {
                    let c = match self.nodes.get(ci) {
                        Some(c) => c,
                        None => continue,
                    };
                    if c.is_active {
                        arr.push(c.value.clone());
                    }
                }
                Value::Array(arr)
            }
            other => other.clone(),
        };
        self.nodes[idx].value = rebuilt;
    }

    pub fn delete_node(&mut self, path: &[String]) -> Result<(), String> {
        if path.is_empty() {
            return Err("Cannot delete root node".to_string());
        }
        self.save_to_undo();
        let parent_path = &path[..path.len() - 1];
        let child_key = &path[path.len() - 1];
        let parent_idx = match find_node_by_path(&self.nodes, parent_path) {
            Some(i) => i,
            None => return Err("Parent node not found".to_string()),
        };
        let child_idx = match self.nodes[parent_idx].children.iter().position(|&ci| {
            self.nodes
                .get(ci)
                .map(|c| c.path.last().map(|s| s == child_key).unwrap_or(false))
                .unwrap_or(false)
        }) {
            Some(p) => self.nodes[parent_idx].children[p],
            None => return Err("Child not found".to_string()),
        };
        // Tombstone: remove from parent's children, set is_active=false, clear value
        let pos_in_children = self.nodes[parent_idx]
            .children
            .iter()
            .position(|&c| c == child_idx)
            .unwrap();
        self.nodes[parent_idx].children.remove(pos_in_children);
        if let Some(child) = self.nodes.get_mut(child_idx) {
            child.is_active = false;
            child.value = Value::Null;
            child.children.clear();
        }
        // Renumber remaining children paths for array parents (active + disabled)
        crate::format::renumber_array_children(&mut self.nodes, parent_idx);
        self.sync_value_from_active_children(parent_idx);
        let mut cur = parent_idx;
        while !self.nodes[cur].path.is_empty() {
            let ancestor_path = self.nodes[cur].path[..self.nodes[cur].path.len() - 1].to_vec();
            match find_node_by_path(&self.nodes, &ancestor_path) {
                Some(ai) => {
                    self.sync_value_from_active_children(ai);
                    cur = ai;
                }
                None => break,
            }
        }
        self.all_nodes_cache.retain(|n| !n.path.starts_with(path));
        // Track deleted node's position in flattened list before rebuild
        let deleted_flat_pos = self.flattened_nodes.iter().position(|n| n.path == path);
        self.rebuild_flattened();
        // Adjust selected: if deleted node was before cursor, shift cursor back
        if let Some(del_pos) = deleted_flat_pos {
            if del_pos < self.selected {
                self.selected = self.selected.saturating_sub(1);
            }
        }
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
        let parent_idx = match find_node_by_path(&self.nodes, parent_path) {
            Some(i) => i,
            None => return Err("Parent node not found".to_string()),
        };
        let (segment, new_value_for_parent): (String, Value) = {
            let parent = &self.nodes[parent_idx];
            match &parent.value {
                Value::Object(map) => {
                    let k = key
                        .as_ref()
                        .ok_or_else(|| "Key required for Object".to_string())?
                        .clone();
                    if map.contains_key(&k) {
                        return Err(format!("Key already exists: {}", k));
                    }
                    let mut m = map.clone();
                    m.insert(k.clone(), value.clone());
                    (k, Value::Object(m))
                }
                Value::Array(arr) => {
                    // Path segment must be unique across ALL children, including
                    // inactive (commented) ones that retain their original index.
                    // Using arr.len() (active count) can collide with an existing
                    // index when earlier items are commented out.
                    let next_idx = parent
                        .children
                        .iter()
                        .filter_map(|&ci| self.nodes.get(ci))
                        .filter_map(|c| c.path.last())
                        .filter_map(|s| s.parse::<usize>().ok())
                        .max()
                        .map(|m| m + 1)
                        .unwrap_or(0)
                        .max(arr.len());
                    (next_idx.to_string(), {
                        let mut a = arr.clone();
                        a.push(value.clone());
                        Value::Array(a)
                    })
                }
                Value::Null => {
                    // initialize
                    if let Some(k) = key.as_ref() {
                        let mut m = serde_json::Map::new();
                        m.insert(k.clone(), value.clone());
                        (k.clone(), Value::Object(m))
                    } else {
                        let a = vec![value.clone()];
                        ("0".to_string(), Value::Array(a))
                    }
                }
                _ => return Err("Parent is not Object/Array/Null".to_string()),
            }
        };
        // schema-driven init for null parent
        let final_parent_value = if self.nodes[parent_idx].value.is_null() {
            if let Some(schema) = &self.schema {
                if let Some(t) = crate::schema_util::find_sub_schema(schema, parent_path)
                    .and_then(|s| s.get("type"))
                    .and_then(|v| v.as_str())
                {
                    if t == "array" && key.is_none() {
                        let a = vec![value.clone()];
                        Value::Array(a)
                    } else if t == "object" && key.is_some() {
                        let mut m = serde_json::Map::new();
                        m.insert(key.unwrap(), value.clone());
                        Value::Object(m)
                    } else {
                        new_value_for_parent
                    }
                } else {
                    new_value_for_parent
                }
            } else {
                new_value_for_parent
            }
        } else {
            new_value_for_parent
        };

        self.nodes[parent_idx].value = final_parent_value.clone();
        let mut child_path = parent_path.to_vec();
        child_path.push(segment.clone());
        let new_idx = self.nodes.len();
        self.nodes.push(AnnotatedNode {
            value: value.clone(),
            is_active: true,
            comments: Vec::new(),
            children: Vec::new(),
            path: child_path.clone(),
        });
        self.nodes[parent_idx].children.push(new_idx);

        // Expand parent in cache
        if let Some(pn) = self
            .flattened_nodes
            .iter_mut()
            .find(|n| n.path == parent_path)
        {
            pn.expanded = true;
        }
        self.rebuild_flattened();
        if let Some(pos) = self
            .flattened_nodes
            .iter()
            .position(|n| n.path == child_path)
        {
            self.selected = pos;
        }
        Ok(())
    }

    pub fn insert_child_node_at(
        &mut self,
        parent_path: &[String],
        child_index: usize,
        key: Option<String>,
        value: Value,
    ) -> Result<(), String> {
        self.save_to_undo();
        let parent_idx = match find_node_by_path(&self.nodes, parent_path) {
            Some(i) => i,
            None => return Err("Parent node not found".to_string()),
        };
        let (segment, new_value_for_parent): (String, Value) = {
            let parent = &self.nodes[parent_idx];
            match &parent.value {
                Value::Object(map) => {
                    let k = key
                        .as_ref()
                        .ok_or_else(|| "Key required for Object".to_string())?
                        .clone();
                    if map.contains_key(&k) {
                        return Err(format!("Key already exists: {}", k));
                    }
                    let mut m = map.clone();
                    m.insert(k.clone(), value.clone());
                    (k, Value::Object(m))
                }
                Value::Array(arr) => {
                    let next_idx = parent
                        .children
                        .iter()
                        .filter_map(|&ci| self.nodes.get(ci))
                        .filter_map(|c| c.path.last())
                        .filter_map(|s| s.parse::<usize>().ok())
                        .max()
                        .map(|m| m + 1)
                        .unwrap_or(0)
                        .max(arr.len());
                    (next_idx.to_string(), {
                        let mut a = arr.clone();
                        a.push(value.clone());
                        Value::Array(a)
                    })
                }
                Value::Null => {
                    if let Some(k) = key.as_ref() {
                        let mut m = serde_json::Map::new();
                        m.insert(k.clone(), value.clone());
                        (k.clone(), Value::Object(m))
                    } else {
                        let a = vec![value.clone()];
                        ("0".to_string(), Value::Array(a))
                    }
                }
                _ => return Err("Parent is not Object/Array/Null".to_string()),
            }
        };

        let final_parent_value = if self.nodes[parent_idx].value.is_null() {
            if let Some(schema) = &self.schema {
                if let Some(t) = crate::schema_util::find_sub_schema(schema, parent_path)
                    .and_then(|s| s.get("type"))
                    .and_then(|v| v.as_str())
                {
                    if t == "array" && key.is_none() {
                        let a = vec![value.clone()];
                        Value::Array(a)
                    } else if t == "object" && key.is_some() {
                        let mut m = serde_json::Map::new();
                        m.insert(key.unwrap(), value.clone());
                        Value::Object(m)
                    } else {
                        new_value_for_parent
                    }
                } else {
                    new_value_for_parent
                }
            } else {
                new_value_for_parent
            }
        } else {
            new_value_for_parent
        };

        self.nodes[parent_idx].value = final_parent_value;
        let mut child_path = parent_path.to_vec();
        child_path.push(segment);
        let new_idx = self.nodes.len();
        self.nodes.push(AnnotatedNode {
            value: value.clone(),
            is_active: true,
            comments: Vec::new(),
            children: Vec::new(),
            path: child_path.clone(),
        });

        let target_pos = child_index.min(self.nodes[parent_idx].children.len());
        self.nodes[parent_idx].children.insert(target_pos, new_idx);

        if self.nodes[parent_idx].value.is_object() {
            self.sync_value_from_active_children(parent_idx);
            self.key_order_changed = true;
        } else if self.nodes[parent_idx].value.is_array() {
            self.sync_value_from_active_children(parent_idx);
            crate::format::renumber_array_children(&mut self.nodes, parent_idx);
            if let Some(new_last) = self.nodes[new_idx].path.last().cloned() {
                child_path[parent_path.len()] = new_last;
            }
        }

        if let Some(pn) = self
            .flattened_nodes
            .iter_mut()
            .find(|n| n.path == parent_path)
        {
            pn.expanded = true;
        }
        self.rebuild_flattened();
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
        let parent_idx = match find_node_by_path(&self.nodes, parent_path) {
            Some(i) => i,
            None => return,
        };
        let children = &self.nodes[parent_idx].children;
        let cur_pos = children.iter().position(|&ci| {
            self.nodes
                .get(ci)
                .map(|c| c.path.last() == Some(child_key))
                .unwrap_or(false)
        });
        let Some(pos) = cur_pos else { return };
        if pos == 0 {
            return;
        }
        self.save_to_undo();
        self.nodes[parent_idx].children.swap(pos, pos - 1);
        // Rebuild parent value from children order, compute new_path
        let mut new_path = node.path.clone();
        match &self.nodes[parent_idx].value {
            Value::Array(_) => {
                let i = child_key.parse::<usize>().unwrap_or(0);
                new_path[node.path.len() - 1] = (i.saturating_sub(1)).to_string();
            }
            Value::Object(_) => {
                new_path = node.path.clone();
            }
            _ => {}
        }
        self.sync_value_from_active_children(parent_idx);
        // Renumber children paths for array parents (active + disabled)
        crate::format::renumber_array_children(&mut self.nodes, parent_idx);
        self.key_order_changed = true;
        self.rebuild_flattened();
        if let Some(p) = self.flattened_nodes.iter().position(|n| n.path == new_path) {
            self.selected = p;
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
        let parent_idx = match find_node_by_path(&self.nodes, parent_path) {
            Some(i) => i,
            None => return,
        };
        let children = &self.nodes[parent_idx].children;
        let cur_pos = children.iter().position(|&ci| {
            self.nodes
                .get(ci)
                .map(|c| c.path.last() == Some(child_key))
                .unwrap_or(false)
        });
        let Some(pos) = cur_pos else { return };
        if pos + 1 >= children.len() {
            return;
        }
        self.save_to_undo();
        self.nodes[parent_idx].children.swap(pos, pos + 1);
        // Rebuild parent value from children order, compute new_path
        let mut new_path = node.path.clone();
        match &self.nodes[parent_idx].value {
            Value::Array(_) => {
                let i = child_key.parse::<usize>().unwrap_or(0);
                new_path[node.path.len() - 1] = (i + 1).to_string();
            }
            Value::Object(_) => {
                new_path = node.path.clone();
            }
            _ => {}
        }
        self.sync_value_from_active_children(parent_idx);
        // Renumber children paths for array parents (active + disabled)
        crate::format::renumber_array_children(&mut self.nodes, parent_idx);
        self.key_order_changed = true;
        self.rebuild_flattened();
        if let Some(p) = self.flattened_nodes.iter().position(|n| n.path == new_path) {
            self.selected = p;
        }
    }

    pub fn toggle_comment(&mut self) -> Result<(), String> {
        let node = match self.selected_node() {
            Some(n) => n.clone(),
            None => return Err("No node selected".to_string()),
        };
        if node.path.is_empty() {
            return Err("Cannot toggle root node".to_string());
        }
        self.save_to_undo();
        if let Some(idx) = find_node_by_path(&self.nodes, &node.path) {
            let n = &mut self.nodes[idx];
            n.is_active = !n.is_active;
        } else {
            return Err("Node not found in nodes vec".to_string());
        }
        self.rebuild_flattened();
        if let Some(pos) = self
            .flattened_nodes
            .iter()
            .position(|n| n.path == node.path)
        {
            self.selected = pos;
        }
        Ok(())
    }

    fn expand_ancestors_and_navigate(&mut self, target_path: &[String]) {
        let mut ancestors = Vec::new();
        for i in 1..target_path.len() {
            ancestors.push(target_path[..i].to_vec());
        }

        for p in ancestors {
            if let Some(node) = self.flattened_nodes.iter_mut().find(|n| n.path == p) {
                node.expanded = true;
            } else {
                self.flattened_nodes.push(UiNode {
                    path: p,
                    depth: 0,
                    key: String::new(),
                    value_display: String::new(),
                    value_type: ValueType::Null,
                    node_type: NodeType::Leaf,
                    expanded: true,
                    is_disabled_comment: false,
                    has_comment: false,
                    comment_preview: None,
                });
            }
        }

        self.rebuild_flattened();

        if let Some(pos) = self
            .flattened_nodes
            .iter()
            .position(|n| n.path == target_path)
        {
            self.selected = pos;
            if self.selected < self.scroll_offset
                || self.selected >= self.scroll_offset + self.viewport_height
            {
                self.scroll_offset = self.selected.saturating_sub(self.viewport_height / 2);
            }
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
        self.collect_search_nodes(self.root, &mut all_nodes);

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
            self.expand_ancestors_and_navigate(&target.path);
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
        self.collect_search_nodes(self.root, &mut all_nodes);

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
        self.collect_search_nodes(self.root, &mut all_nodes);

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
        let Some(target) = matches.first().map(|(_, node)| node) else {
            return;
        };

        self.expand_ancestors_and_navigate(&target.path);

        self.update_search_match_stats(query);
    }

    fn collect_search_nodes(&self, idx: usize, all_nodes: &mut Vec<SearchNode>) {
        let Some(node) = self.nodes.get(idx) else {
            return;
        };
        let key = node
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| "root".to_string());
        let value_str = match &node.value {
            Value::String(s) => s.clone(),
            Value::Object(_) | Value::Array(_) => String::new(),
            v => v.to_string(),
        };
        all_nodes.push(SearchNode {
            path: node.path.clone(),
            key,
            value: value_str,
        });
        for &child_idx in &node.children {
            self.collect_search_nodes(child_idx, all_nodes);
        }
    }

    pub fn selected_node(&self) -> Option<&UiNode> {
        self.flattened_nodes.get(self.selected)
    }

    /// Scroll the tooltip by delta, clamping to the scroll limit.
    pub fn scroll_tooltip(&mut self, delta: isize) {
        let limit = self.tooltip.scroll_limit(self);
        self.tooltip.scroll(delta);
        if let Some(limit) = limit {
            self.tooltip.clamp_scroll(limit);
        }
    }

    /// Get completion suggestions based on the current cursor node.
    pub fn completions_at_cursor(&self) -> Vec<CompletionItem> {
        if let Some(node) = self.selected_node() {
            self.completions_for_path(&node.path)
        } else {
            Vec::new()
        }
    }

    /// Get completion suggestions at a specific path.
    pub fn completions_for_path(&self, path: &[String]) -> Vec<CompletionItem> {
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
        crate::flatten::count_nodes_per_level(&self.nodes, self.root, 0, &mut counts);

        // 2. Calculate the maximum depth that can be expanded within the limit
        let mut max_expand_depth = 0;
        let mut current_total = counts[0];

        for d in 0..counts.len() {
            if d + 1 >= counts.len() {
                max_expand_depth = d;
                break;
            }
            if current_total + counts[d + 1] > limit {
                break;
            }
            current_total += counts[d + 1];
            max_expand_depth = d;
        }

        let mut dummy_prev_nodes = Vec::new();
        self.collect_expansion_paths_nodes(
            self.root,
            Vec::new(),
            0,
            max_expand_depth,
            &mut dummy_prev_nodes,
        );

        // 4. Rebuild the flattened view based on the dummy node list
        self.flattened_nodes = crate::flatten::rebuild_flattened(
            &self.nodes,
            self.root,
            &dummy_prev_nodes,
            self.show_child_counts,
            self.schema.as_ref(),
            self.format,
        );
    }

    fn collect_expansion_paths_nodes(
        &self,
        idx: usize,
        path: Vec<String>,
        depth: usize,
        max_depth: usize,
        nodes: &mut Vec<UiNode>,
    ) {
        if depth > max_depth {
            return;
        }

        let Some(node) = self.nodes.get(idx) else {
            return;
        };
        let is_container = !node.children.is_empty();
        if !is_container {
            return;
        }

        nodes.push(UiNode {
            path: path.clone(),
            depth,
            key: String::new(),
            value_display: String::new(),
            value_type: ValueType::Null,
            node_type: NodeType::Leaf,
            expanded: true,
            is_disabled_comment: false,
            has_comment: false,
            comment_preview: None,
        });

        for &child_idx in &node.children {
            if let Some(child) = self.nodes.get(child_idx) {
                let mut child_path = path.clone();
                child_path.push(child.path.last().cloned().unwrap_or_default());
                self.collect_expansion_paths_nodes(
                    child_idx,
                    child_path,
                    depth + 1,
                    max_depth,
                    nodes,
                );
            }
        }
    }

    pub fn handle_key_event(&mut self, event: crossterm::event::KeyEvent) -> crate::action::Action {
        let prev_selected = self.selected;
        let prev_edit_mode = self.edit_mode.clone();
        let action = self.handle_key_event_inner(event);
        if self.selected != prev_selected
            || !matches!(self.edit_mode, EditMode::Normal)
            || !matches!(prev_edit_mode, EditMode::Normal)
        {
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
                // Single-key shortcuts (no modifiers)
                if !event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    if let KeyCode::Char(c) = event.code {
                        match c.to_ascii_lowercase() {
                            's' => {
                                return Action::Save {
                                    format: self.format,
                                };
                            }
                            'u' => {
                                self.undo();
                                return Action::Noop;
                            }
                            'r' => {
                                self.redo();
                                return Action::Noop;
                            }
                            't' => {
                                self.show_type_hints = !self.show_type_hints;
                                return Action::Noop;
                            }
                            'k' => {
                                self.show_child_counts = !self.show_child_counts;
                                self.rebuild_flattened();
                                return Action::Noop;
                            }
                            'i' => {
                                crate::edit::trigger_add_sibling_after(self);
                                return Action::Noop;
                            }
                            'f' => {
                                self.edit_mode = EditMode::SearchPrompt {
                                    buffer: String::new(),
                                    cursor_pos: 0,
                                };
                                self.search_query = Some(String::new());
                                return Action::Noop;
                            }
                            '/' => {
                                if let Err(e) = self.toggle_comment() {
                                    self.set_status(e);
                                }
                                return Action::Noop;
                            }
                            _ => {}
                        }
                    }
                }

                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    match event.code {
                        KeyCode::Up => {
                            crate::navigate::move_to_prev_sibling(self);
                            return Action::Noop;
                        }
                        KeyCode::Down => {
                            crate::navigate::move_to_next_sibling(self);
                            return Action::Noop;
                        }
                        _ => {}
                    }
                }

                if event.modifiers.contains(KeyModifiers::ALT) {
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
                        self.edit_mode = EditMode::Help {
                            scroll_offset: 0,
                            max_offset: 0,
                        };
                        return Action::Noop;
                    }
                    KeyCode::Up => {
                        self.tooltip.reset_scroll();
                        crate::navigate::move_up(self)
                    }
                    KeyCode::Down => {
                        self.tooltip.reset_scroll();
                        crate::navigate::move_down(self)
                    }
                    KeyCode::PageUp => {
                        if self.tooltip.is_active(self) {
                            self.scroll_tooltip(-1);
                            return Action::Noop;
                        } else {
                            crate::navigate::page_up(self)
                        }
                    }
                    KeyCode::PageDown => {
                        if self.tooltip.is_active(self) {
                            self.scroll_tooltip(1);
                            return Action::Noop;
                        } else {
                            crate::navigate::page_down(self)
                        }
                    }
                    KeyCode::Left => {
                        self.tooltip.reset_scroll();
                        crate::navigate::collapse_current(self)
                    }
                    KeyCode::Right => {
                        self.tooltip.reset_scroll();
                        crate::navigate::expand_or_move_to_last_child(self)
                    }
                    KeyCode::Char(' ') => {
                        self.tooltip.reset_scroll();
                        crate::navigate::toggle_expand(self)
                    }
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
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
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
                            && (self.nodes != self.original_nodes || self.key_order_changed);
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
            EditMode::Help {
                scroll_offset,
                max_offset,
            } => match event.code {
                KeyCode::PageUp => {
                    if *scroll_offset > 0 {
                        *scroll_offset = scroll_offset.saturating_sub(1);
                    }
                    return Action::Noop;
                }
                KeyCode::PageDown => {
                    if *scroll_offset < *max_offset {
                        *scroll_offset = scroll_offset.saturating_add(1);
                    }
                    return Action::Noop;
                }
                _ => {
                    self.edit_mode = EditMode::Normal;
                    return Action::Noop;
                }
            },
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
                        let byte_idx = char_to_byte_index(buffer, *cursor_pos);
                        buffer.insert(byte_idx, c);
                        *cursor_pos += 1;
                        self.search_query = Some(buffer.clone());
                        run_realtime = true;
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            let byte_idx = char_to_byte_index(buffer, *cursor_pos);
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
                            format: self.format,
                        };
                    }
                }
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    return Action::SaveAndQuit {
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
                        let byte_idx = char_to_byte_index(buffer, *cursor_pos);
                        buffer.insert(byte_idx, c);
                        *cursor_pos += 1;
                    }
                    KeyCode::Backspace => {
                        if *cursor_pos > 0 {
                            *cursor_pos -= 1;
                            let byte_idx = char_to_byte_index(buffer, *cursor_pos);
                            buffer.remove(byte_idx);
                        } else if matches!(self.edit_mode, EditMode::TextPrompt { .. }) {
                            if is_backspace_repeat {
                                return Action::Noop;
                            }
                            // Transition from TextPrompt to RenameKeyPrompt when buffer is empty
                            if let Some(node) =
                                self.selected_node().cloned().filter(|n| !n.path.is_empty())
                            {
                                let mut parent_path = node.path.clone();
                                let Some(original_key) = parent_path.pop() else {
                                    return Action::Noop;
                                };

                                // Check if parent is an object
                                let is_parent_object = if parent_path.is_empty() {
                                    self.nodes[self.root].value.is_object()
                                } else {
                                    self.node_at_path_as_value(&parent_path)
                                        .map(|v| v.is_object())
                                        .unwrap_or(false)
                                };

                                if is_parent_object {
                                    let current_value = self
                                        .node_at_path_as_value(&node.path)
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
            EditMode::Dropdown {
                options,
                descriptions: _,
                selected,
                scroll_offset,
                filter_buffer,
                filtered_indices,
            } => match event.code {
                KeyCode::Enter => {
                    if !filtered_indices.is_empty() {
                        let original_idx = filtered_indices[*selected];
                        *selected = original_idx;
                    }
                    crate::edit::apply_edit(self);
                }
                KeyCode::Esc => crate::edit::cancel_edit(self),
                KeyCode::Up if *selected > 0 => {
                    self.tooltip.reset_scroll();
                    *selected -= 1;
                    if *selected < *scroll_offset {
                        *scroll_offset = *selected;
                    }
                }
                KeyCode::Down if *selected + 1 < filtered_indices.len() => {
                    self.tooltip.reset_scroll();
                    *selected += 1;
                    let visible = self.dropdown_visible_items;
                    if visible > 0 && *selected >= *scroll_offset + visible {
                        *scroll_offset = *selected + 1 - visible;
                    }
                }
                KeyCode::PageUp => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(-1);
                    }
                }
                KeyCode::PageDown => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(1);
                    }
                }
                KeyCode::Char(c) => {
                    filter_buffer.push(c);
                    *filtered_indices = options
                        .iter()
                        .enumerate()
                        .filter(|(_, opt)| {
                            opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                        })
                        .map(|(i, _)| i)
                        .collect();
                    *selected = 0;
                    *scroll_offset = 0;
                }
                KeyCode::Backspace => {
                    if !filter_buffer.is_empty() {
                        filter_buffer.pop();
                        *filtered_indices = options
                            .iter()
                            .enumerate()
                            .filter(|(_, opt)| {
                                if filter_buffer.is_empty() {
                                    true
                                } else {
                                    opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                                }
                            })
                            .map(|(i, _)| i)
                            .collect();
                        *selected = 0;
                        *scroll_offset = 0;
                    }
                }
                _ => {}
            },
            EditMode::NewKeyDropdown {
                options,
                selected,
                scroll_offset,
                filter_buffer,
                cursor_pos,
                filtered_indices,
                ..
            } => match event.code {
                KeyCode::Enter => {
                    if !filtered_indices.is_empty() {
                        let original_idx = filtered_indices[*selected];
                        *selected = original_idx;
                    }
                    crate::edit::apply_edit(self);
                }
                KeyCode::Esc => crate::edit::cancel_edit(self),
                KeyCode::Up if *selected > 0 => {
                    self.tooltip.reset_scroll();
                    *selected -= 1;
                    if *selected < *scroll_offset {
                        *scroll_offset = *selected;
                    }
                }
                KeyCode::Down if *selected + 1 < filtered_indices.len() => {
                    self.tooltip.reset_scroll();
                    *selected += 1;
                    let visible = self.dropdown_visible_items;
                    if visible > 0 && *selected >= *scroll_offset + visible {
                        *scroll_offset = *selected + 1 - visible;
                    }
                }
                KeyCode::PageUp => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(-1);
                    }
                }
                KeyCode::PageDown => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(1);
                    }
                }
                KeyCode::Char(c) => {
                    let byte_idx = char_to_byte_index(filter_buffer, *cursor_pos);
                    filter_buffer.insert(byte_idx, c);
                    *cursor_pos += 1;
                    *filtered_indices = options
                        .iter()
                        .enumerate()
                        .filter(|(_, opt)| {
                            opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                        })
                        .map(|(i, _)| i)
                        .collect();
                    *selected = 0;
                    *scroll_offset = 0;
                }
                KeyCode::Backspace => {
                    if *cursor_pos > 0 {
                        *cursor_pos -= 1;
                        let byte_idx = char_to_byte_index(filter_buffer, *cursor_pos);
                        filter_buffer.remove(byte_idx);
                        *filtered_indices = options
                            .iter()
                            .enumerate()
                            .filter(|(_, opt)| {
                                if filter_buffer.is_empty() {
                                    true
                                } else {
                                    opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                                }
                            })
                            .map(|(i, _)| i)
                            .collect();
                        *selected = 0;
                        *scroll_offset = 0;
                    }
                }
                KeyCode::Left if *cursor_pos > 0 => {
                    *cursor_pos -= 1;
                }
                KeyCode::Right if *cursor_pos < filter_buffer.chars().count() => {
                    *cursor_pos += 1;
                }
                KeyCode::Home => {
                    *cursor_pos = 0;
                }
                KeyCode::End => {
                    *cursor_pos = filter_buffer.chars().count();
                }
                _ => {}
            },
            EditMode::OneOfVariantDropdown {
                options,
                selected,
                scroll_offset,
                filter_buffer,
                cursor_pos,
                filtered_indices,
                ..
            } => match event.code {
                KeyCode::Enter => {
                    if !filtered_indices.is_empty() {
                        let original_idx = filtered_indices[*selected];
                        *selected = original_idx;
                    }
                    crate::edit::apply_oneof_variant(self);
                }
                KeyCode::Esc => {
                    // Cancel: restore null value
                    self.edit_mode = EditMode::Normal;
                }
                KeyCode::Up if *selected > 0 => {
                    self.tooltip.reset_scroll();
                    *selected -= 1;
                    if *selected < *scroll_offset {
                        *scroll_offset = *selected;
                    }
                }
                KeyCode::Down if *selected + 1 < filtered_indices.len() => {
                    self.tooltip.reset_scroll();
                    *selected += 1;
                    let visible = self.dropdown_visible_items;
                    if visible > 0 && *selected >= *scroll_offset + visible {
                        *scroll_offset = *selected + 1 - visible;
                    }
                }
                KeyCode::PageUp => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(-1);
                    }
                }
                KeyCode::PageDown => {
                    if self.tooltip.is_active(self) {
                        self.scroll_tooltip(1);
                    }
                }
                KeyCode::Char(c) => {
                    let byte_idx = char_to_byte_index(filter_buffer, *cursor_pos);
                    filter_buffer.insert(byte_idx, c);
                    *cursor_pos += 1;
                    *filtered_indices = options
                        .iter()
                        .enumerate()
                        .filter(|(_, opt)| {
                            opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                        })
                        .map(|(i, _)| i)
                        .collect();
                    *selected = 0;
                    *scroll_offset = 0;
                }
                KeyCode::Backspace => {
                    if *cursor_pos > 0 {
                        *cursor_pos -= 1;
                        let byte_idx = char_to_byte_index(filter_buffer, *cursor_pos);
                        filter_buffer.remove(byte_idx);
                        *filtered_indices = options
                            .iter()
                            .enumerate()
                            .filter(|(_, opt)| {
                                if filter_buffer.is_empty() {
                                    true
                                } else {
                                    opt.to_lowercase().contains(&filter_buffer.to_lowercase())
                                }
                            })
                            .map(|(i, _)| i)
                            .collect();
                        *selected = 0;
                        *scroll_offset = 0;
                    }
                }
                KeyCode::Left if *cursor_pos > 0 => {
                    *cursor_pos -= 1;
                }
                KeyCode::Right if *cursor_pos < filter_buffer.chars().count() => {
                    *cursor_pos += 1;
                }
                KeyCode::Home => {
                    *cursor_pos = 0;
                }
                KeyCode::End => {
                    *cursor_pos = filter_buffer.chars().count();
                }
                _ => {}
            },
        }
        Action::Noop
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
        assert_eq!(state.active_value(), json!({"b": 2}));
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

        assert_eq!(state.active_value()["new_key"], "value");
        // Root + 1 child
        assert_eq!(state.flattened_nodes.len(), 2);
    }

    #[test]
    fn test_add_child_to_array() {
        let data = json!([]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.add_child_node(&[], None, json!(1)).unwrap();
        state.add_child_node(&[], None, json!(2)).unwrap();

        assert_eq!(state.active_value(), json!([1, 2]));
    }

    #[test]
    fn test_delete_node_from_object() {
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.delete_node(&["a".to_string()]).unwrap();

        assert_eq!(state.active_value(), json!({"b": 2}));
        assert_eq!(state.flattened_nodes.len(), 2); // root + "b"
    }

    #[test]
    fn test_add_empty_array_item_persists_on_save() {
        // C3.1: adding an item to an empty array then saving must not drop it.
        // Regression guard: relies on root-array block serialization (C5.1-1).
        let data = crate::format::parse("[]", crate::format::Format::Yaml).unwrap();
        let mut state = EditorState::new(
            data,
            crate::format::Format::Yaml,
            None,
            Some("[]".to_string()),
        );
        state.selected = 0; // root
        crate::edit::trigger_add_child(&mut state); // adds unedited (null) item, enters edit
        let out = crate::format::serialize_annotated(
            &state.nodes,
            state.root,
            crate::format::Format::Yaml,
        )
        .unwrap();
        // Root array must serialize as a block array, NOT a mapping like "0: ".
        assert!(out.contains("- "), "expected block array, got:\n{}", out);
        assert!(
            !out.contains("0:"),
            "root array must not serialize as mapping:\n{}",
            out
        );
        // Round trip must preserve the item.
        let reparsed = crate::format::parse(&out, crate::format::Format::Yaml).unwrap();
        assert_eq!(
            reparsed,
            json!([null]),
            "added item must persist, got: {}",
            reparsed
        );
    }

    #[test]
    fn test_delete_node_from_array_and_shift() {
        let data = json!([1, 2, 3]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.delete_node(&["1".to_string()]).unwrap(); // delete '2'

        assert_eq!(state.active_value(), json!([1, 3]));
        // Check flattened paths
        assert_eq!(state.flattened_nodes[1].path, vec!["0".to_string()]);
        assert_eq!(state.flattened_nodes[2].path, vec!["1".to_string()]);
    }

    #[test]
    fn test_delete_node_adjusts_selected_before_cursor() {
        let data = json!([1, 2, 3]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        // Root(0), [0](1), [1](2), [2](3)
        state.selected = 2; // select [1] = 2
        state.delete_node(&["0".to_string()]).unwrap(); // delete [0] = 1
        // After: [0]=2, [1]=3. deleted_flat_pos=1 < selected=2 → selected shifts to 1
        assert_eq!(state.selected, 1);
        assert_eq!(state.flattened_nodes[1].path, vec!["0".to_string()]);
        assert_eq!(state.flattened_nodes[1].value_display, "2");
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
    fn test_addable_keys_reeappear_after_delete() {
        use crate::edit::get_addable_keys_with_descriptions;
        let data = json!({"b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.schema = Some(json!({
            "type": "object",
            "properties": {
                "a": { "type": "integer" },
                "b": { "type": "integer" },
                "c": { "type": "integer" }
            }
        }));

        // Initial check: "a" and "c" are addable, "b" is not (already exists)
        let addable = get_addable_keys_with_descriptions(&state, &[]);
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        assert!(keys.contains(&"a".to_string()));
        assert!(!keys.contains(&"b".to_string()));
        assert!(keys.contains(&"c".to_string()));

        // Add "a"
        state
            .add_child_node(&[], Some("a".to_string()), json!(1))
            .unwrap();
        let addable = get_addable_keys_with_descriptions(&state, &[]);
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        assert!(!keys.contains(&"a".to_string()));

        // Delete "a"
        state.delete_node(&["a".to_string()]).unwrap();
        let addable = get_addable_keys_with_descriptions(&state, &[]);
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        // "a" should reappear
        assert!(keys.contains(&"a".to_string()));
        assert!(!keys.contains(&"b".to_string()));
        assert!(keys.contains(&"c".to_string()));
    }

    #[test]
    fn test_parent_value_synced_after_delete() {
        let data = json!({"arr": [1, 2, 3]});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Delete element 2 (index 1) in arr
        state
            .delete_node(&["arr".to_string(), "1".to_string()])
            .unwrap();

        // The parent array ("arr")'s value should be updated to [1, 3]
        let arr_node = state.node_at_path(&["arr".to_string()]).unwrap();
        assert_eq!(arr_node.value, json!([1, 3]));

        // The root's value should also be updated to {"arr": [1, 3]}
        let root_node = state.node_at_path(&[]).unwrap();
        assert_eq!(root_node.value, json!({"arr": [1, 3]}));
    }

    #[test]
    fn test_readd_node_value_edit_targets_active_node() {
        // Regression: after deleting a node and re-adding the same key, the
        // tombstoned (inactive) node coexists with the new active one. A value
        // edit must target the ACTIVE node, otherwise the typed value is lost
        // (written to the invisible tombstone) and not saved.
        let data = json!({"age": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.delete_node(&["age".to_string()]).unwrap();
        // Re-add "age": now an inactive tombstone AND a new active node share the path.
        state
            .add_child_node(&[], Some("age".to_string()), Value::Null)
            .unwrap();

        if let Some(v) = state.node_at_path_mut_value(&["age".to_string()]) {
            *v = json!(42);
        }
        // The active value must reflect the edit (not stay null / tombstoned).
        assert_eq!(state.active_value()["age"], json!(42));
    }

    #[test]
    fn test_undo_redo_basic() {
        let data = json!({"a": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // 1. Save initial state
        state.save_to_undo();

        // 2. Modify data (simulate change by adding a child with value override)
        state.node_at_path_mut(&["a".to_string()]).unwrap().value = json!(2);
        state.rebuild_flattened();
        state.selected = 10;

        // 3. Perform Undo
        state.undo();
        assert_eq!(state.active_value(), json!({"a": 1}));
        assert_eq!(state.selected, 0);

        // 4. Perform Redo
        state.redo();
        assert_eq!(state.active_value(), json!({"a": 2}));
        assert_eq!(state.selected, 10);
    }

    #[test]
    fn test_undo_redo_delete() {
        let mut state = EditorState::new(json!({"a": 1, "b": 2}), Format::Json, None, None);

        // 1. Delete "a" (save_to_undo is called internally in delete_node)
        state.delete_node(&["a".to_string()]).unwrap();
        assert_eq!(state.active_value(), json!({"b": 2}));

        // 2. Perform Undo
        state.undo();
        assert_eq!(state.active_value(), json!({"a": 1, "b": 2}));

        // 3. Perform Redo
        state.redo();
        assert_eq!(state.active_value(), json!({"b": 2}));
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
        state.node_at_path_mut(&["a".to_string()]).unwrap().value = json!(2);
        state.rebuild_flattened();
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
        state.node_at_path_mut(&["a".to_string()]).unwrap().value = json!(2);
        state.rebuild_flattened();
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
        // '?' should switch to Help mode from Normal
        assert!(matches!(state.edit_mode, EditMode::Normal));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty()));
        assert!(matches!(state.edit_mode, EditMode::Help { .. }));

        // Any key (other than PgUp/PgDn) should switch back to Normal mode
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
        assert_eq!(state.active_value(), json!([2, 1, 3]));
        assert_eq!(state.selected, 1); // Follow the moved node
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Move down: [1, 2, 3]
        state.move_node_down();
        assert_eq!(state.active_value(), json!([1, 2, 3]));
        assert_eq!(state.selected, 2);
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Move down again: [1, 3, 2]
        state.move_node_down();
        assert_eq!(state.active_value(), json!([1, 3, 2]));
        assert_eq!(state.selected, 3);
        assert_eq!(state.flattened_nodes[state.selected].value_display, "2");

        // Undo
        state.undo();
        assert_eq!(state.active_value(), json!([1, 2, 3]));
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
        let av = state.active_value();
        let keys: Vec<_> = av.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["b", "a", "c"]);
        assert_eq!(state.selected, 1); // Follow "b"
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Move down: {"a": 1, "b": 2, "c": 3}
        state.move_node_down();
        let av = state.active_value();
        let keys: Vec<_> = av.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["a", "b", "c"]);
        assert_eq!(state.selected, 2);
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Move down again: {"a": 1, "c": 3, "b": 2}
        state.move_node_down();
        let av = state.active_value();
        let keys: Vec<_> = av.as_object().unwrap().keys().collect::<Vec<_>>();
        assert_eq!(keys, vec!["a", "c", "b"]);
        assert_eq!(state.selected, 3);
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        // Undo
        state.undo();
        let av = state.active_value();
        let keys: Vec<_> = av.as_object().unwrap().keys().collect::<Vec<_>>();
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
        let event = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE);
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
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
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
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
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
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));

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
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
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
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        state.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(state.search_total_matches, 0);

        state.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::Normal));
        assert_eq!(
            state.search_query, None,
            "Search query should be cleared immediately if no matches"
        );

        // Scenario 3: SearchPrompt with EMPTY buffer + ESC -> Reset immediately
        state.handle_key_event(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
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
        assert_eq!(state.active_value(), serde_json::json!({"key": "val"}));
        assert_eq!(state.flattened_nodes[1].key, "key");
    }

    #[test]
    fn test_backspace_preserve_value_on_key_rename_and_cancel() {
        use crate::edit::{apply_edit, cancel_edit};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // 1. Value with complex structure/scalar to verify preservation
        let data = serde_json::json!({"port": 8080});
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Select the "port" node and enter TextPrompt
        state.selected = 1;
        state.edit_mode = EditMode::TextPrompt {
            buffer: "".to_string(),
            cursor_pos: 0,
        };

        // Backspace to transition to RenameKeyPrompt
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(matches!(state.edit_mode, EditMode::RenameKeyPrompt { .. }));

        // Cancel edit with Esc/cancel_edit
        cancel_edit(&mut state);
        assert_eq!(state.edit_mode, EditMode::Normal);
        assert_eq!(state.active_value(), serde_json::json!({"port": 8080}));

        // Re-enter edit mode and transition to RenameKeyPrompt again
        state.last_backspace_time = None;
        state.edit_mode = EditMode::TextPrompt {
            buffer: "".to_string(),
            cursor_pos: 0,
        };
        state.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

        // Rename key to "server_port" and apply
        if let EditMode::RenameKeyPrompt {
            buffer, cursor_pos, ..
        } = &mut state.edit_mode
        {
            *buffer = "server_port".to_string();
            *cursor_pos = 11;
        }
        apply_edit(&mut state);

        // Verify key was renamed while value 8080 remained completely preserved
        assert_eq!(
            state.active_value(),
            serde_json::json!({"server_port": 8080})
        );
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
        assert_eq!(
            state.active_value(),
            serde_json::json!({"config": {"theme": "dark"}})
        );
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
        {
            let key_path = vec!["nested".to_string(), "key".to_string()];
            if let Some(val) = state.node_at_path_mut(&key_path) {
                val.value = serde_json::json!("new_value");
            }
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
        assert_eq!(state.active_value(), json!({"b": 2}));

        // Reset and test 'Ctrl+d'
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1;
        let event = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        state.handle_key_event(event);
        assert_eq!(state.active_value(), json!({"a": 1, "b": 2}));
    }

    #[test]
    fn test_handle_key_event_scroll_on_mode_change() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1;
        state.scroll_to_selected = false;

        state.edit_mode = EditMode::Dropdown {
            options: vec!["opt1".to_string()],
            descriptions: vec![None],
            selected: 0,
            scroll_offset: 0,
            filter_buffer: String::new(),
            filtered_indices: vec![0],
        };

        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        state.handle_key_event(event);

        assert!(matches!(state.edit_mode, EditMode::Normal));
        assert!(state.scroll_to_selected);
    }

    #[test]
    fn test_toggle_comment() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let original_text = "a: 1\nb: 2\nc: 3\n";
        let data = crate::format::parse(original_text, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original_text.to_string()));

        state.selected = 2; // root=0, a=1, b=2, c=3
        assert_eq!(state.flattened_nodes[state.selected].key, "b");

        let event = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        state.handle_key_event(event);

        assert_eq!(state.active_value(), serde_json::json!({"a": 1, "c": 3}));
        assert!(state.flattened_nodes[2].is_disabled_comment);
        assert_eq!(state.flattened_nodes[2].key, "b");

        state.handle_key_event(event);

        assert_eq!(
            state.active_value(),
            serde_json::json!({"a": 1, "b": 2, "c": 3})
        );
        assert!(!state.flattened_nodes[2].is_disabled_comment);
        assert_eq!(state.flattened_nodes[2].key, "b");
    }

    #[test]
    #[ignore] // Phase 3: from-text reparse will correct ordering
    fn test_toggle_middle_comment_orders() {
        use crate::format::parse;
        let yaml = "services:\n  hermes:\n    build:\n      dockerfile: Dockerfile.hermes\n    image: localhost/hermes-agent:latest\n    container_name: hermes\n    # restart: unless-stopped\n    command: gateway run\n    network_mode: host\n    environment:\n      CONTAINER_HOST: unix:///run/docker/docker.sock\n    ports:\n      - \"9119:9119\"\n    volumes:\n      - hermes-data:/opt/data\n      - /mnt/wsl/projects:/projects\n";
        let data = parse(yaml, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml.to_string()));
        // expand services
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["services".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);
        // expand hermes
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["services".to_string(), "hermes".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);
        let hc = |st: &EditorState| -> Vec<String> {
            st.flattened_nodes
                .iter()
                .filter(|n| n.path.len() == 3 && n.path[0] == "services" && n.path[1] == "hermes")
                .map(|n| n.key.clone())
                .collect()
        };
        let idx = state
            .flattened_nodes
            .iter()
            .position(|n| {
                n.path
                    == vec![
                        "services".to_string(),
                        "hermes".to_string(),
                        "image".to_string(),
                    ]
            })
            .expect("image node");
        state.selected = idx;
        state.toggle_comment().unwrap();
        let after = hc(&state);
        assert_eq!(
            after,
            vec![
                "build",
                "image",
                "container_name",
                "restart",
                "command",
                "network_mode",
                "environment",
                "ports",
                "volumes"
            ],
            "commenting a MIDDLE key must not reorder sibling keys (IndexMap swap_remove bug)"
        );
    }

    #[test]
    fn test_toggle_comment_array_multiple() {
        // Bug: Commenting apple then banana should comment both, not skip to cherry
        let yaml = "items:\n  - apple\n  - banana\n  - cherry\n  - date\n  - elderberry\n";
        let data = crate::format::parse(yaml, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml.to_string()));

        // Expand items array
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // Select first array item (apple)
        let first_item = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .expect("first item should exist");
        state.selected = first_item;

        // Comment apple
        state.toggle_comment().unwrap();

        // Find next non-disabled item (should be banana)
        let banana_idx = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment);
        assert!(
            banana_idx.is_some(),
            "banana should still exist after commenting apple"
        );
        state.selected = banana_idx.unwrap();

        // Comment banana
        state.toggle_comment().unwrap();

        // Verify both items are commented (2 disabled nodes)
        let commented: Vec<_> = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .collect();
        assert_eq!(commented.len(), 2, "Should have 2 commented items");

        // Verify active value has only 3 items (cherry, date, elderberry)
        let active = state.active_value();
        let arr = active.pointer("/items").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 3, "Array should have 3 items after commenting 2");
    }

    #[test]
    fn test_toggle_comment_array_last() {
        // Bug: Commenting last item fails with "Node value not found"
        let yaml = "items:\n  - apple\n  - banana\n  - cherry\n";
        let data = crate::format::parse(yaml, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml.to_string()));

        // Expand items
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // Find last non-disabled item (cherry)
        let last_item = state
            .flattened_nodes
            .iter()
            .rposition(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .expect("last item should exist");
        state.selected = last_item;

        // Comment should succeed
        let result = state.toggle_comment();
        assert!(
            result.is_ok(),
            "Commenting last item should succeed, got: {:?}",
            result.err()
        );

        // Verify an item is commented
        let disabled_count = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .count();
        assert_eq!(disabled_count, 1, "One item should be disabled");
    }

    #[test]
    fn test_toggle_comment_array_uncomment_no_duplicate() {
        // Bug: Uncommenting causes duplicate entries (comment + value coexist)
        // With node-based toggle: uncomment just flips is_active back.
        // Since parse_annotated already creates disabled nodes from "# - apple",
        // uncommenting means: is_active = true → apple reappears in active_value.
        // NOTE: The old data-removal model inserted the value back into data.
        // The new node model simply flips is_active. The disabled node was already
        // in the vec from parse_annotated, so flip makes it active.
        let yaml = "items:\n  - apple\n  - banana\n  - cherry\n";
        let data = crate::format::parse(yaml, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml.to_string()));

        // Expand items
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // All items are active (yaml had no commented items in data)
        // Comment apple first, then uncomment it
        let apple_active = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment);
        assert!(apple_active.is_some(), "apple should be active");
        state.selected = apple_active.unwrap();

        // Comment apple
        state.toggle_comment().unwrap();

        // Now find the disabled apple
        let apple_disabled = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment);
        assert!(apple_disabled.is_some(), "apple should now be disabled");
        state.selected = apple_disabled.unwrap();

        // Uncomment
        state.toggle_comment().unwrap();

        // Verify active value has all 3 items
        let active = state.active_value();
        let arr = active.pointer("/items").unwrap().as_array().unwrap();
        assert_eq!(arr.len(), 3, "Array should have 3 items after uncommenting");

        // Verify no disabled nodes remain
        let disabled_count = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .count();
        assert_eq!(disabled_count, 0, "No disabled nodes should remain");
    }

    #[test]
    fn test_toggle_comment_array_jsonc_rendering() {
        // Bug: After commenting first item, second item disappears from UI
        let jsonc = "{\n  \"items\": [\n    \"apple\",\n    \"banana\",\n    \"cherry\"\n  ]\n}\n";
        let data = crate::format::parse(jsonc, Format::Jsonc).unwrap();
        let mut state = EditorState::new(data, Format::Jsonc, None, Some(jsonc.to_string()));

        // Expand items
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // Count visible (non-disabled) items before
        let visible_before = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .count();
        assert_eq!(visible_before, 3);

        // Select and comment first item (apple)
        let first_item = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .expect("first item should exist");
        state.selected = first_item;
        state.toggle_comment().unwrap();

        // Count visible items after - banana should still be visible
        let visible_after = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .count();
        assert_eq!(
            visible_after, 2,
            "banana and cherry should still be visible after commenting apple"
        );
    }

    /// Phase 2: save with no edits must preserve above/inline comments via original_text merge.
    #[test]
    fn test_save_preserves_comments_yaml() {
        let original = r#"# System Configuration
app:
  name: clise # Application name
  # Server settings
  server:
    host: 127.0.0.1
    port: 8080
"#;
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();

        assert!(
            out.contains("# System Configuration"),
            "file header lost: {}",
            out
        );
        assert!(
            out.contains("# Application name") || out.contains("name: clise # Application name"),
            "inline comment lost: {}",
            out
        );
        assert!(
            out.contains("# Server settings"),
            "above comment lost: {}",
            out
        );
    }

    /// Phase 2: disabled array items must survive save via original_text merge.
    #[test]
    fn test_save_preserves_disabled_yaml() {
        let original = "items:\n  - apple\n  # - banana\n  - cherry\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();

        assert!(
            out.contains("# - banana") || out.contains("#- banana"),
            "disabled item lost on save: {}",
            out
        );
        assert!(out.contains("apple"), "active apple lost: {}", out);
        assert!(out.contains("cherry"), "active cherry lost: {}", out);
    }

    /// Phase 2: JSONC comments preserved on save with original_text.
    #[test]
    fn test_save_preserves_jsonc() {
        let original = r#"{
  // above a
  "a": 1, // inline a
  "b": 2
}
"#;
        let data = crate::format::parse(original, Format::Jsonc).unwrap();
        let state = EditorState::new(data, Format::Jsonc, None, Some(original.to_string()));

        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Jsonc).unwrap();

        assert!(
            out.contains("// above a") || out.contains("//above a"),
            "above comment lost: {}",
            out
        );
        assert!(
            out.contains("// inline a") || out.contains("//inline a"),
            "inline comment lost: {}",
            out
        );
    }

    #[test]
    fn test_rename_key_then_serialize() {
        use crate::edit::apply_edit;
        let original = "# header\n# above a\na: 1  # inline a\nb: 2\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        // rename key "a" to "c"
        state.selected = 1; // "a" node
        state.edit_mode = EditMode::RenameKeyPrompt {
            parent_path: vec![],
            original_key: "a".to_string(),
            buffer: "c".to_string(),
            cursor_pos: 1,
            value: serde_json::json!(1),
        };
        apply_edit(&mut state);

        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();

        assert!(
            out.contains("c: 1"),
            "renamed key not serialized correctly: {}",
            out
        );
        assert!(out.contains("# above a"), "comment preserved: {}", out);
        assert!(out.contains("# inline a"), "comment preserved: {}", out);
    }

    #[test]
    fn test_reorder_then_serialize() {
        let original = "a: 1\nb: 2\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        // Move "b" (selected = 2) up
        state.selected = 2; // "b" node
        state.move_node_up();

        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();

        assert_eq!(out, "b: 2\na: 1\n");
    }

    #[test]
    fn test_add_array_item_with_leading_commented_item_no_path_collision() {
        // Regression: when the first array item is commented out, it retains its
        // original index (items.0, inactive) while active items keep items.1/2.
        // Adding a new item previously used arr.len() (active count) as the new
        // index, producing items.2 — a DUPLICATE of the existing cherry node.
        // That made find_node_by_path (used by start_edit) resolve to cherry,
        // so the user would edit the wrong node.
        let original = "items:\n  # - apple\n  - banana\n  - cherry\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::edit::trigger_add_child(&mut state);

        // No two nodes may share the same path.
        let mut paths: Vec<Vec<String>> = state.nodes.iter().map(|n| n.path.clone()).collect();
        let before = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(before, paths.len(), "duplicate node paths after add");

        // Selection must land on the newly added node (unique index), not cherry.
        let sel = state.selected_node().expect("no selection").path.clone();
        assert_eq!(sel, vec!["items".to_string(), "3".to_string()]);

        // The commented item and both active items survive; new empty item appended.
        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();
        assert!(out.contains("# - apple"), "commented item lost:\n{}", out);
        assert!(out.contains("banana"), "banana lost:\n{}", out);
        assert!(out.contains("cherry"), "cherry lost:\n{}", out);
    }

    #[test]
    fn test_delete_disabled_node_from_array() {
        let original = "items:\n  - apple\n  # - banana\n  - cherry\n  - date\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        // Expand items
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // Find disabled banana node
        let banana_idx = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .expect("disabled banana should exist");
        state.selected = banana_idx;
        let banana_path = state.selected_node().unwrap().path.clone();

        // Delete disabled banana
        state.delete_node(&banana_path).unwrap();

        // No disabled nodes remain in items
        let disabled_count = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .count();
        assert_eq!(disabled_count, 0, "no disabled nodes should remain");

        // Active items: apple, cherry, date (3 items)
        let active_items: Vec<_> = state
            .flattened_nodes
            .iter()
            .filter(|n| n.path.len() == 2 && n.path[0] == "items" && !n.is_disabled_comment)
            .map(|n| n.value_display.clone())
            .collect();
        assert_eq!(active_items.len(), 3, "should have 3 active items");

        // Serialized output: banana gone, apple/cherry/date remain
        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();
        assert!(out.contains("apple"), "apple lost:\n{}", out);
        assert!(out.contains("cherry"), "cherry lost:\n{}", out);
        assert!(!out.contains("banana"), "banana still present:\n{}", out);
    }

    #[test]
    fn test_delete_disabled_node_from_object() {
        let yaml = "a: 1\n# b: 2\nc: 3\n";
        let data = crate::format::parse(yaml, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml.to_string()));

        // Find disabled "b" node
        let b_idx = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["b".to_string()] && n.is_disabled_comment)
            .expect("disabled b should exist");
        state.selected = b_idx;
        let b_path = state.selected_node().unwrap().path.clone();

        state.delete_node(&b_path).unwrap();

        // b gone, a and c remain
        assert!(
            !state.flattened_nodes.iter().any(|n| n.path == b_path),
            "b should be gone"
        );
        let out =
            crate::format::serialize_annotated(&state.nodes, state.root, Format::Yaml).unwrap();
        assert!(out.contains("a: 1"), "a lost:\n{}", out);
        assert!(out.contains("c: 3"), "c lost:\n{}", out);
    }

    #[test]
    fn test_move_disabled_node_up_down_in_array() {
        let original = "items:\n  - apple\n  # - banana\n  - cherry\n  - date\n";
        let data = crate::format::parse(original, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(original.to_string()));

        // Expand items
        state.selected = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["items".to_string()])
            .unwrap();
        crate::navigate::toggle_expand(&mut state);

        // Disabled banana is at position 1 in children
        let banana_idx = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .expect("disabled banana should exist");
        state.selected = banana_idx;

        // Move down: banana should swap with cherry (active)
        state.move_node_down();

        // After move, banana should still be disabled, now after cherry in children
        let banana_still = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .expect("disabled banana should still exist after move");
        assert!(banana_still > banana_idx, "banana should have moved down");

        // Move up back
        state.selected = banana_still;
        state.move_node_up();

        let banana_back = state
            .flattened_nodes
            .iter()
            .position(|n| n.path.len() == 2 && n.path[0] == "items" && n.is_disabled_comment)
            .expect("disabled banana should exist after move back");
        assert!(
            banana_back < banana_still,
            "banana should have moved back up"
        );
    }

    #[test]
    fn test_backspace_key_rename_preserves_original_value() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let data = json!({"test": 123});
        let mut state = EditorState::new(data, Format::Yaml, None, None);

        state.selected = 1; // "test" node
        crate::edit::start_edit(&mut state);

        if let EditMode::TextPrompt {
            ref mut buffer,
            ref mut cursor_pos,
        } = state.edit_mode
        {
            buffer.clear();
            *cursor_pos = 0;
        } else {
            panic!("Expected TextPrompt mode");
        }

        // Backspace to trigger key rename transition
        let event = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        state.handle_key_event(event);

        assert!(matches!(state.edit_mode, EditMode::RenameKeyPrompt { .. }));

        if let EditMode::RenameKeyPrompt {
            ref mut buffer,
            ref mut cursor_pos,
            ..
        } = state.edit_mode
        {
            *buffer = "renamed_test".to_string();
            *cursor_pos = buffer.chars().count();
        }

        crate::edit::apply_edit(&mut state);

        assert_eq!(state.active_value()["renamed_test"], 123);
    }
}
