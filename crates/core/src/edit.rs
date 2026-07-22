use crate::schema_util::{
    collect_addable_keys, collect_property_completions, detect_combo_key, find_sub_schema,
    find_sub_schema_for_value, oneof_variants, resolve_ref, resolve_schema_type_and_default,
    schema_type_includes,
};
use crate::state::{CompletionItem, CompletionKind, EditMode, EditorState, NodeType};
use crate::util::{is_ambiguous_string, to_json_pointer};
use serde_json::Value;

pub fn start_edit(state: &mut EditorState) {
    start_edit_impl(state, false);
}

pub fn start_edit_cleared(state: &mut EditorState) {
    start_edit_impl(state, true);
}

fn handle_enum_edit(state: &mut EditorState, path: &[String], current_value: &Value) -> bool {
    let sub_schema = match state.schema.as_ref().and_then(|s| find_sub_schema(s, path)) {
        Some(s) => s,
        None => return false,
    };
    let enum_values = match sub_schema.get("enum").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return false,
    };

    let options: Vec<String> = enum_values
        .iter()
        .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
        .collect();

    let descriptions: Vec<Option<String>> = sub_schema
        .get("enumDescriptions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            options
                .iter()
                .enumerate()
                .map(|(i, _)| arr.get(i).and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| vec![None; options.len()]);

    let selected = options
        .iter()
        .position(|opt| opt == &current_value.as_str().unwrap_or(&current_value.to_string()))
        .unwrap_or(0);

    state.edit_mode = EditMode::Dropdown {
        options: options.clone(),
        descriptions,
        selected,
        scroll_offset: 0,
        filter_buffer: String::new(),
        filtered_indices: (0..options.len()).collect(),
    };
    true
}

fn handle_oneof_variant_edit(
    state: &mut EditorState,
    path: &[String],
    current_value: &Value,
    clear_value: bool,
) -> bool {
    if *current_value != Value::Null || clear_value {
        return false;
    }
    let sub_schema = match state.schema.as_ref().and_then(|s| find_sub_schema(s, path)) {
        Some(s) => s,
        None => return false,
    };

    let combo_key = match detect_combo_key(sub_schema) {
        Some(key) if key != "allOf" => Some(key),
        _ => None,
    };
    let key = match combo_key {
        Some(k) => k,
        None => return false,
    };

    match sub_schema.get(key).and_then(|v| v.as_array()) {
        Some(a) if a.len() > 1 => {}
        _ => return false,
    };

    let variants = oneof_variants(sub_schema);
    if variants.len() <= 1 {
        return false;
    }

    let parent_path = path[..path.len().saturating_sub(1)].to_vec();
    let target_key = path.last().cloned().unwrap_or_default();
    let options: Vec<String> = variants.iter().map(|v| v.label.clone()).collect();
    let descriptions: Vec<Option<String>> =
        variants.iter().map(|v| v.description.clone()).collect();
    let count = options.len();
    state.edit_mode = EditMode::OneOfVariantDropdown {
        parent_path,
        target_key,
        options,
        descriptions,
        selected: 0,
        scroll_offset: 0,
        filter_buffer: String::new(),
        cursor_pos: 0,
        filtered_indices: (0..count).collect(),
    };
    true
}

fn start_edit_impl(state: &mut EditorState, clear_value: bool) {
    let (path, node_type, is_disabled_comment) = match state.selected_node() {
        Some(n) => (n.path.clone(), n.node_type.clone(), n.is_disabled_comment),
        None => return,
    };

    if is_disabled_comment {
        return;
    }

    let current_value = state
        .node_at_path_as_value(&path)
        .cloned()
        .unwrap_or(Value::Null);

    // Special handling for Boolean
    let is_bool = current_value.as_bool();
    if let Some(b) = is_bool.filter(|_| !clear_value) {
        state.save_to_undo();
        let new_value = Value::Bool(!b);
        if let Some(v) = state.node_at_path_mut_value(&path) {
            *v = new_value;
        }
        state.edit_mode = EditMode::Normal;
        state.rebuild_flattened();
        return;
    }

    // Check schema for enum
    if handle_enum_edit(state, &path, &current_value) {
        return;
    }

    // Check for oneOf/anyOf variant picker when value is null
    if handle_oneof_variant_edit(state, &path, &current_value, clear_value) {
        return;
    }

    // For other types, enter edit mode
    match &node_type {
        NodeType::Leaf => {
            // Null value + object-typed schema: auto-init as empty object.
            // Handles $ref/patternProperties entries like networks.<name>.
            // find_sub_schema already resolves $ref recursively, so we check
            // the resolved schema directly instead of looking for raw $ref.
            if current_value.is_null() && !clear_value {
                if let Some(schema) = &state.schema {
                    if let Some(sub) = find_sub_schema(schema, &path) {
                        let resolved = resolve_ref(schema, sub);
                        if schema_type_includes(resolved, "object")
                            || resolved.get("properties").is_some()
                        {
                            state.save_to_undo();
                            if let Some(v) = state.node_at_path_mut_value(&path) {
                                *v = Value::Object(serde_json::Map::new());
                            }
                            state.rebuild_flattened();
                            return;
                        }
                    }
                }
            }

            let buffer = if clear_value {
                "".to_string()
            } else {
                match current_value {
                    Value::String(s) => {
                        if is_ambiguous_string(&s) {
                            format!("\"{}\"", s)
                        } else {
                            s.clone()
                        }
                    }
                    Value::Number(n) => n.to_string(),
                    _ => "".to_string(),
                }
            };
            state.edit_mode = EditMode::TextPrompt {
                buffer: buffer.clone(),
                cursor_pos: buffer.chars().count(),
            };
        }
        _ => {
            if clear_value && path.len() > 0 {
                let parent_path = path[..path.len() - 1].to_vec();
                let is_parent_object = if parent_path.is_empty() {
                    state.nodes[state.root].value.is_object()
                } else {
                    state
                        .node_at_path_as_value(&parent_path)
                        .map(|v| v.is_object())
                        .unwrap_or(false)
                };

                if is_parent_object {
                    let original_key = path.last().unwrap().clone();
                    let mut new_buffer = original_key.clone();
                    if !new_buffer.is_empty() {
                        new_buffer.pop();
                    }
                    let new_cursor_pos = new_buffer.chars().count();

                    // Check if schema has options for this parent object
                    let addable = get_addable_keys_with_descriptions(state, &parent_path);
                    if !addable.is_empty() {
                        let mut options: Vec<String> =
                            addable.iter().map(|(k, _)| k.clone()).collect();
                        let mut descs: Vec<Option<String>> =
                            addable.into_iter().map(|(_, d)| d).collect();
                        if !options.contains(&original_key) {
                            let idx = options.partition_point(|k| k < &original_key);
                            options.insert(idx, original_key.clone());
                            descs.insert(idx, None);
                        }

                        let filtered_indices: Vec<usize> = options
                            .iter()
                            .enumerate()
                            .filter(|(_, opt)| {
                                if new_buffer.is_empty() {
                                    true
                                } else {
                                    opt.to_lowercase().contains(&new_buffer.to_lowercase())
                                }
                            })
                            .map(|(i, _)| i)
                            .collect();

                        state.edit_mode = EditMode::NewKeyDropdown {
                            parent_path,
                            temp_key: original_key.clone(),
                            options,
                            descriptions: descs,
                            selected: 0,
                            scroll_offset: 0,
                            filter_buffer: new_buffer,
                            cursor_pos: new_cursor_pos,
                            filtered_indices,
                        };
                    } else {
                        state.edit_mode = EditMode::RenameKeyPrompt {
                            parent_path,
                            original_key,
                            buffer: new_buffer,
                            cursor_pos: new_cursor_pos,
                            value: current_value.clone(),
                        };
                    }
                }
            }
        }
    }
}

pub fn get_completions_for_path(state: &EditorState, path: &[String]) -> Vec<CompletionItem> {
    let schema = match &state.schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Get the current value at this path for value-aware resolution
    let current_value = state.node_at_path_as_value(path);

    let sub_schema = match find_sub_schema_for_value(schema, path, current_value) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut completions = Vec::new();

    // 1. Enum suggestions
    if let Some(enums) = sub_schema.get("enum").and_then(|v| v.as_array()) {
        for val in enums {
            completions.push(CompletionItem {
                label: val.as_str().unwrap_or(&val.to_string()).to_string(),
                value: val.clone(),
                kind: CompletionKind::Enum,
                detail: None,
            });
        }
    }

    // 2. Boolean suggestions
    if schema_type_includes(sub_schema, "boolean") {
        completions.push(CompletionItem {
            label: "true".to_string(),
            value: Value::Bool(true),
            kind: CompletionKind::Boolean,
            detail: None,
        });
        completions.push(CompletionItem {
            label: "false".to_string(),
            value: Value::Bool(false),
            kind: CompletionKind::Boolean,
            detail: None,
        });
    }

    // 3. Default suggestions
    if let Some(def) = sub_schema.get("default") {
        completions.push(CompletionItem {
            label: format!("Default: {}", def),
            value: def.clone(),
            kind: CompletionKind::Default,
            detail: Some("Schema default value".to_string()),
        });
    }

    // 4. Property suggestions
    //    For oneOf/anyOf object variants, collect properties from all matching object variants
    collect_property_completions(schema, sub_schema, &mut completions);

    completions
}

pub fn apply_completion(state: &mut EditorState, path: &[String], item: &CompletionItem) {
    state.save_to_undo();
    if let Some(v) = state.node_at_path_mut_value(path) {
        *v = item.value.clone();
    }
    state.rebuild_flattened();
}

/// Apply the selected oneOf/anyOf variant.
/// Writes a typed placeholder value and returns to edit mode.
pub fn apply_oneof_variant(state: &mut EditorState) {
    let (parent_path, target_key, options, selected, filtered_indices) = match &state.edit_mode {
        EditMode::OneOfVariantDropdown {
            parent_path,
            target_key,
            options,
            selected,
            filtered_indices,
            ..
        } => (
            parent_path.clone(),
            target_key.clone(),
            options.clone(),
            *selected,
            filtered_indices.clone(),
        ),
        _ => return,
    };

    // Guard: if filter is active but nothing matches, or selected out of bounds
    if filtered_indices.is_empty() || selected >= options.len() {
        state.edit_mode = EditMode::Normal;
        return;
    }

    let label = options[selected].clone();
    state.edit_mode = EditMode::Normal;

    // Determine the type from the selected label and write a placeholder
    let placeholder = if label.contains("String") || label.contains("path") || label.contains("URL")
    {
        Value::String(String::new())
    } else if label.contains("Object") || label.contains("config") || label.contains("detailed") {
        Value::Object(serde_json::Map::new())
    } else if label.contains("Array") {
        Value::Array(Vec::new())
    } else if label.contains("Boolean") {
        Value::Bool(false)
    } else if label.contains("Number") {
        Value::Number(serde_json::Number::from(0))
    } else {
        // Default: try to infer from the oneOf/anyOf variants
        Value::Object(serde_json::Map::new())
    };

    // Write the placeholder
    let full_path = if parent_path.is_empty() {
        vec![target_key.clone()]
    } else {
        let mut p = parent_path.clone();
        p.push(target_key.clone());
        p
    };
    state.save_to_undo();
    if let Some(v) = state.node_at_path_mut_value(&full_path) {
        *v = placeholder;
    }
    state.rebuild_flattened();

    // Move cursor to the node
    if let Some(pos) = state
        .flattened_nodes
        .iter()
        .position(|n| n.path == full_path)
    {
        state.selected = pos;
    }

    // Start edit on the now-non-null value only for leaf types
    // For objects/arrays, user will expand and add children via Enter
    let is_leaf = matches!(
        state
            .flattened_nodes
            .iter()
            .find(|n| n.path == full_path)
            .map(|n| &n.node_type),
        Some(NodeType::Leaf)
    );
    if is_leaf {
        start_edit(state);
    }
}

fn get_default_value_from_schema(schema: &Value, path: &[String]) -> Value {
    // Use value-aware resolver with null to prefer object/array variants
    if let Some(sub) = find_sub_schema_for_value(schema, path, None) {
        let (def, t) = resolve_schema_type_and_default(schema, sub);
        if let Some(val) = def {
            return val;
        }
        if let Some(t_str) = t {
            return match t_str.as_str() {
                "array" => Value::Array(Vec::new()),
                "object" => Value::Object(serde_json::Map::new()),
                "boolean" => Value::Bool(false),
                "string" => Value::String(String::new()),
                "number" | "integer" => Value::Number(serde_json::Number::from(0)),
                _ => Value::Null,
            };
        }
    }
    Value::Null
}

/// True when the schema at `path` resolves to type "boolean".
fn is_boolean_schema(state: &EditorState, path: &[String]) -> bool {
    if let Some(schema) = &state.schema {
        if let Some(sub) = find_sub_schema(schema, path) {
            let resolved = resolve_ref(schema, sub);
            if let Some(t) = resolved.get("type").and_then(|v| v.as_str()) {
                return t == "boolean";
            }
        }
    }
    false
}

/// Returns true if `parent_path` is an object that already contains `key`,
/// excluding the placeholder key `exclude_key` (temp key or original key).
fn key_exists_in_parent(
    state: &EditorState,
    parent_path: &[String],
    key: &str,
    exclude_key: &str,
) -> bool {
    if key == exclude_key {
        return false;
    }
    if let Some(Value::Object(map)) = state.node_at_path_as_value(parent_path) {
        return map.contains_key(key);
    }
    false
}

fn replace_key_in_json_object(parent: &mut Value, old_key: &str, new_key: &str, val: Value) {
    if let Value::Object(map) = parent {
        if map.remove(old_key).is_some() {
            map.insert(new_key.to_string(), val);
        }
    }
}

fn rename_key_in_nodes(
    state: &mut EditorState,
    parent_path: Vec<String>,
    temp_key: String,
    key: String,
) {
    let val = if let Some(schema) = &state.schema {
        let mut child_path = parent_path.clone();
        child_path.push(key.clone());
        get_default_value_from_schema(schema, &child_path)
    } else {
        Value::Null
    };

    if let Some(parent_val) = state.node_at_path_mut_value(&parent_path) {
        replace_key_in_json_object(parent_val, &temp_key, &key, val.clone());
    }

    // Rename the key in state.nodes and state.all_nodes_cache
    let mut old_prefix = parent_path.clone();
    old_prefix.push(temp_key);

    let mut new_prefix = parent_path.clone();
    new_prefix.push(key.clone());

    for node in &mut state.all_nodes_cache {
        if node.path.starts_with(&old_prefix) {
            let suffix = node.path[old_prefix.len()..].to_vec();
            let mut new_path = new_prefix.clone();
            new_path.extend(suffix);
            node.path = new_path;
        }
    }

    for node in &mut state.nodes {
        if node.path.starts_with(&old_prefix) {
            let suffix = node.path[old_prefix.len()..].to_vec();
            let mut new_path = new_prefix.clone();
            new_path.extend(suffix);
            node.path = new_path;
        }
    }

    if let Some(child_node) = state.node_at_path_mut(&new_prefix) {
        child_node.value = val;
    }

    state.rebuild_flattened();

    // Move cursor to the newly changed key node
    let mut target_path = parent_path.clone();
    target_path.push(key.clone());
    if let Some(pos) = state
        .flattened_nodes
        .iter()
        .position(|n| n.path == target_path)
    {
        state.selected = pos;
        // Improvement 2: focus lands on the VALUE field.
        // - Boolean values toggle via Enter in Normal mode, so do NOT auto-toggle
        //   a freshly added boolean key; just land on the node.
        // - Other types (enum/oneOf/string/number/object) open the proper editor.
        if is_boolean_schema(state, &target_path) {
            state.edit_mode = EditMode::Normal;
        } else {
            start_edit(state);
        }
    }
}

pub fn apply_edit(state: &mut EditorState) {
    match &state.edit_mode {
        EditMode::NewKeyDropdown {
            parent_path,
            temp_key,
            options,
            selected,
            filtered_indices,
            filter_buffer,
            ..
        } => {
            // save_to_undo already called in trigger_add_child
            let key = if filtered_indices.is_empty() {
                // No match — use typed text as the key name
                filter_buffer.clone()
            } else {
                options[*selected].clone()
            };

            if key.is_empty() {
                state.edit_mode = EditMode::Normal;
                state.undo();
                state.pop_redo();
                return;
            }

            // Improvement 1: block duplicate key, warn, do not register
            if key_exists_in_parent(state, &parent_path, &key, &temp_key) {
                state.edit_mode = EditMode::Normal;
                state.undo(); // remove temp node added by trigger_add_child
                state.pop_redo();
                state.set_status(format!("Key '{}' already exists", key));
                return;
            }

            let parent_path = parent_path.clone();
            let temp_key = temp_key.clone();
            state.edit_mode = EditMode::Normal;
            rename_key_in_nodes(state, parent_path, temp_key, key);
            return;
        }
        EditMode::NewKeyPrompt {
            parent_path,
            temp_key,
            buffer,
            ..
        } => {
            let key = buffer.trim().to_string();

            if key.is_empty() {
                state.edit_mode = EditMode::Normal;
                state.undo();
                state.pop_redo();
            } else if key_exists_in_parent(state, &parent_path, &key, &temp_key) {
                // Improvement 1: block duplicate key, warn, do not register
                state.edit_mode = EditMode::Normal;
                state.undo();
                state.pop_redo();
                state.set_status(format!("Key '{}' already exists", key));
            } else {
                let parent_path = parent_path.clone();
                let temp_key = temp_key.clone();
                state.edit_mode = EditMode::Normal;
                rename_key_in_nodes(state, parent_path, temp_key, key);
            }
        }
        EditMode::RenameKeyPrompt {
            parent_path,
            original_key,
            buffer,
            value,
            ..
        } => {
            let new_key = buffer.trim().to_string();
            let parent_path = parent_path.clone();
            let original_key = original_key.clone();
            let preserved_value = value.clone();
            state.edit_mode = EditMode::Normal;

            if new_key.is_empty() {
                // Delete the key-value pair
                state.save_to_undo();
                if let Some(parent) = state.node_at_path_mut_value(&parent_path) {
                    if let Value::Object(map) = parent {
                        map.remove(&original_key);
                    }
                }
                state.rebuild_flattened();
                if let Some(pos) = state
                    .flattened_nodes
                    .iter()
                    .position(|n| n.path == parent_path)
                {
                    state.selected = pos;
                }
            } else if new_key == original_key {
                // no-op (unchanged)
            } else if key_exists_in_parent(state, &parent_path, &new_key, &original_key) {
                // Improvement 1: cannot rename onto an existing key
                state.set_status(format!("Cannot rename: key '{}' already exists", new_key));
                // selection already restored above via edit_mode=Normal; do NOT mutate
            } else {
                // Rename the key (existing body, unchanged)
                state.save_to_undo();
                if let Some(parent) = state.node_at_path_mut_value(&parent_path) {
                    if let Value::Object(map) = parent {
                        // Rebuild the map to preserve key order
                        let mut new_map = serde_json::Map::new();
                        for (k, v) in map.iter() {
                            if k == &original_key {
                                new_map.insert(new_key.clone(), preserved_value.clone());
                            } else {
                                new_map.insert(k.clone(), v.clone());
                            }
                        }
                        *map = new_map;
                    }
                }

                // Update paths in all_nodes_cache
                let mut old_prefix = parent_path.clone();
                old_prefix.push(original_key.clone());

                let mut new_prefix = parent_path.clone();
                new_prefix.push(new_key.clone());

                for node in &mut state.all_nodes_cache {
                    if node.path.starts_with(&old_prefix) {
                        let suffix = node.path[old_prefix.len()..].to_vec();
                        let mut new_path = new_prefix.clone();
                        new_path.extend(suffix);
                        node.path = new_path;
                    }
                }

                // Update paths in state.nodes (required for active_value reconstruction)
                for node in &mut state.nodes {
                    if node.path.starts_with(&old_prefix) {
                        let suffix = node.path[old_prefix.len()..].to_vec();
                        let mut new_path = new_prefix.clone();
                        new_path.extend(suffix);
                        node.path = new_path;
                        if node.path == new_prefix {
                            node.value = preserved_value.clone();
                        }
                    }
                }

                if let Some(child_node) = state.node_at_path_mut(&new_prefix) {
                    child_node.value = preserved_value;
                }

                state.rebuild_flattened();

                // Track key rename in state.renamed_keys
                let parent_ptr_str = to_json_pointer(&parent_path);
                let new_key_path = if parent_ptr_str.is_empty() {
                    format!("/{}", new_key)
                } else {
                    format!("{}/{}", parent_ptr_str, new_key)
                };
                let orig_key_path = if parent_ptr_str.is_empty() {
                    format!("/{}", original_key)
                } else {
                    format!("{}/{}", parent_ptr_str, original_key)
                };
                let true_original_key = state
                    .renamed_keys
                    .remove(&orig_key_path)
                    .unwrap_or_else(|| original_key.clone());
                state.renamed_keys.insert(new_key_path, true_original_key);

                // Move cursor to the renamed key
                let mut target_path = parent_path;
                target_path.push(new_key);
                if let Some(pos) = state
                    .flattened_nodes
                    .iter()
                    .position(|n| n.path == target_path)
                {
                    state.selected = pos;
                }
            }
            return;
        }
        _ => {}
    }

    let path = match state.selected_node() {
        Some(n) => n.path.clone(),
        None => {
            state.edit_mode = EditMode::Normal;
            return;
        }
    };

    state.save_to_undo();
    let original_value = state.node_at_path_as_value(&path);

    let new_value = match &state.edit_mode {
        EditMode::TextPrompt { buffer, .. } => {
            let trimmed = buffer.trim();
            let schema_type = if let Some(schema) = &state.schema {
                find_sub_schema(schema, &path).and_then(|s| s.get("type").and_then(|v| v.as_str()))
            } else {
                None
            };

            let has_quotes =
                (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
                    || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2);

            let is_string_target = schema_type == Some("string");

            // Empty buffer + null value: generate schema-aware default
            if trimmed.is_empty() && original_value.is_some_and(|v| v.is_null()) {
                let schema_type_resolved = if let Some(schema) = &state.schema {
                    resolve_schema_type_and_default(
                        schema,
                        find_sub_schema_for_value(schema, &path, Some(&Value::Null))
                            .unwrap_or(schema),
                    )
                    .1
                } else {
                    None
                };
                match schema_type_resolved.as_deref() {
                    Some("object") => Value::Object(serde_json::Map::new()),
                    Some("array") => Value::Array(Vec::new()),
                    Some("boolean") => Value::Bool(false),
                    Some("number") | Some("integer") => Value::Number(0.into()),
                    _ => Value::Null,
                }
            } else if has_quotes {
                // Strip quotes and treat as string when explicitly quoted
                let unquoted = &trimmed[1..trimmed.len() - 1];
                Value::String(unquoted.to_string())
            } else if trimmed == "[]" {
                Value::Array(Vec::new())
            } else if trimmed == "{}" {
                Value::Object(serde_json::Map::new())
            } else if is_string_target {
                // Keep as string when existing value is string or schema targets string type
                Value::String(buffer.clone())
            } else if trimmed == "true" {
                Value::Bool(true)
            } else if trimmed == "false" {
                Value::Bool(false)
            } else {
                match original_value {
                    Some(Value::String(_)) => {
                        if let Ok(n) = buffer.parse::<i64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<u64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<f64>() {
                            Value::Number(
                                serde_json::Number::from_f64(n)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            )
                        } else if buffer == "true" {
                            Value::Bool(true)
                        } else if buffer == "false" {
                            Value::Bool(false)
                        } else if buffer == "null" {
                            Value::Null
                        } else {
                            Value::String(buffer.clone())
                        }
                    }
                    Some(Value::Number(_)) => {
                        if let Ok(n) = buffer.parse::<i64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<u64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<f64>() {
                            Value::Number(
                                serde_json::Number::from_f64(n)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            )
                        } else {
                            Value::String(buffer.clone())
                        }
                    }
                    Some(Value::Bool(_)) => {
                        if buffer == "true" {
                            Value::Bool(true)
                        } else if buffer == "false" {
                            Value::Bool(false)
                        } else {
                            Value::String(buffer.clone())
                        }
                    }
                    _ => {
                        // Inference based on schema or content
                        if schema_type == Some("string") {
                            Value::String(buffer.clone())
                        } else if let Ok(n) = buffer.parse::<i64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<u64>() {
                            Value::Number(serde_json::Number::from(n))
                        } else if let Ok(n) = buffer.parse::<f64>() {
                            Value::Number(
                                serde_json::Number::from_f64(n)
                                    .unwrap_or_else(|| serde_json::Number::from(0)),
                            )
                        } else if buffer == "true" {
                            Value::Bool(true)
                        } else if buffer == "false" {
                            Value::Bool(false)
                        } else if buffer == "null" {
                            Value::Null
                        } else {
                            Value::String(buffer.clone())
                        }
                    }
                }
            }
        }
        EditMode::Dropdown {
            options, selected, ..
        } => {
            let val_str = &options[*selected];
            // Try to parse as bool/number if it looks like one, otherwise string
            if val_str == "true" {
                Value::Bool(true)
            } else if val_str == "false" {
                Value::Bool(false)
            } else if let Ok(n) = val_str.parse::<i64>() {
                Value::Number(serde_json::Number::from(n))
            } else {
                Value::String(val_str.clone())
            }
        }
        _ => return,
    };

    if let Some(v) = state.node_at_path_mut_value(&path) {
        *v = new_value.clone();
    }

    state.edit_mode = EditMode::Normal;
    state.rebuild_flattened();

    if matches!(new_value, Value::Object(_) | Value::Array(_)) {
        if let Some(node_mut) = state.flattened_nodes.iter_mut().find(|n| n.path == path) {
            node_mut.expanded = true;
        }
        state.rebuild_flattened();
    }
}

pub fn cancel_edit(state: &mut EditorState) {
    match &state.edit_mode {
        EditMode::NewKeyDropdown { .. } | EditMode::NewKeyPrompt { .. } => {
            state.edit_mode = EditMode::Normal;
            state.undo();
            state.pop_redo();
        }
        _ => {
            state.edit_mode = EditMode::Normal;
        }
    }
}
pub fn get_addable_keys_with_descriptions(
    state: &EditorState,
    path: &[String],
) -> Vec<(String, Option<String>)> {
    let schema = match &state.schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Get current value at this path for value-aware resolution
    let current_value = state.node_at_path_as_value(path);

    let sub_schema = match find_sub_schema_for_value(schema, path, current_value) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut addable_keys = Vec::new();

    // Get current keys at this path
    let current_keys: std::collections::HashSet<String> =
        if let Some(Value::Object(map)) = state.node_at_path_as_value(path) {
            map.keys().cloned().collect()
        } else {
            std::collections::HashSet::new()
        };

    collect_addable_keys(schema, sub_schema, &current_keys, &mut addable_keys);
    addable_keys.sort_by(|a, b| a.0.cmp(&b.0));
    addable_keys
}

pub fn trigger_add_child(state: &mut EditorState) {
    let (path, node_type) = match state.selected_node() {
        Some(n) => (n.path.clone(), n.node_type.clone()),
        None => return,
    };

    match node_type {
        NodeType::Array { .. } => {
            // Arrays: check schema for items default
            let mut val = Value::Null;
            if let Some(schema) = &state.schema {
                let mut item_path = path.clone();
                item_path.push("*".to_string());
                if let Some(sub) = find_sub_schema(schema, &item_path) {
                    if let Some(def) = sub.get("default") {
                        val = def.clone();
                    }
                }
            }
            let _ = state.add_child_node(&path, None, val);
            start_edit(state);
        }
        NodeType::Object { .. } => {
            let addable = get_addable_keys_with_descriptions(state, &path);

            // Generate a unique temporary key (avoiding duplicates)
            let mut temp_key = "new_key".to_string();
            if let Some(Value::Object(map)) = state.node_at_path_as_value(&path) {
                let mut count = 0;
                while map.contains_key(&temp_key) {
                    count += 1;
                    temp_key = format!("new_key_{}", count);
                }
            }

            // add_child_node handles saving to undo, updating map and nodes, expanding parent, rebuilding and selecting
            let _ = state.add_child_node(&path, Some(temp_key.clone()), Value::Null);

            if !addable.is_empty() {
                let options: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
                let descs: Vec<Option<String>> = addable.into_iter().map(|(_, d)| d).collect();
                let count = options.len();
                state.edit_mode = EditMode::NewKeyDropdown {
                    parent_path: path,
                    temp_key,
                    options,
                    descriptions: descs,
                    selected: 0,
                    scroll_offset: 0,
                    filter_buffer: String::new(),
                    cursor_pos: 0,
                    filtered_indices: (0..count).collect(),
                };
            } else {
                state.edit_mode = EditMode::NewKeyPrompt {
                    parent_path: path,
                    temp_key,
                    buffer: String::new(),
                    cursor_pos: 0,
                };
            }
        }
        NodeType::Leaf => {}
    }
}

pub fn trigger_add_sibling_after(state: &mut EditorState) {
    let sel_node = match state.selected_node() {
        Some(n) => n.clone(),
        None => return,
    };

    if sel_node.path.is_empty() {
        // Root node has no parent/sibling, fall back to adding child
        trigger_add_child(state);
        return;
    }

    let parent_path = sel_node.path[..sel_node.path.len() - 1].to_vec();
    let child_key = &sel_node.path[sel_node.path.len() - 1];

    let parent_node = match state.node_at_path(&parent_path) {
        Some(n) => n.clone(),
        None => return,
    };

    // Calculate insertion index in parent's children (immediately after current selected child)
    let child_index = match parent_node.children.iter().position(|&ci| {
        state
            .nodes
            .get(ci)
            .map(|c| c.path.last() == Some(child_key))
            .unwrap_or(false)
    }) {
        Some(pos) => pos + 1,
        None => parent_node.children.len(),
    };

    if parent_node.value.is_array() {
        let mut val = Value::Null;
        if let Some(schema) = &state.schema {
            let mut item_path = parent_path.clone();
            item_path.push("*".to_string());
            if let Some(sub) = find_sub_schema(schema, &item_path) {
                if let Some(def) = sub.get("default") {
                    val = def.clone();
                }
            }
        }
        let _ = state.insert_child_node_at(&parent_path, child_index, None, val);
        start_edit(state);
    } else if parent_node.value.is_object() {
        let addable = get_addable_keys_with_descriptions(state, &parent_path);

        let mut temp_key = "new_key".to_string();
        if let Some(Value::Object(map)) = state.node_at_path_as_value(&parent_path) {
            let mut count = 0;
            while map.contains_key(&temp_key) {
                count += 1;
                temp_key = format!("new_key_{}", count);
            }
        }

        let _ = state.insert_child_node_at(
            &parent_path,
            child_index,
            Some(temp_key.clone()),
            Value::Null,
        );

        if !addable.is_empty() {
            let options: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
            let descs: Vec<Option<String>> = addable.into_iter().map(|(_, d)| d).collect();
            let count = options.len();
            state.edit_mode = EditMode::NewKeyDropdown {
                parent_path,
                temp_key,
                options,
                descriptions: descs,
                selected: 0,
                scroll_offset: 0,
                filter_buffer: String::new(),
                cursor_pos: 0,
                filtered_indices: (0..count).collect(),
            };
        } else {
            state.edit_mode = EditMode::NewKeyPrompt {
                parent_path,
                temp_key,
                buffer: String::new(),
                cursor_pos: 0,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::schema_util::matches_pattern;
    use serde_json::json;

    #[test]
    fn test_edit_boolean_toggle() {
        let data = json!({"active": true});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "active"
        start_edit(&mut state);

        assert_eq!(state.active_value()["active"], false);
        assert_eq!(state.edit_mode, EditMode::Normal);
    }

    #[test]
    fn test_edit_text_prompt() {
        let data = json!({"name": "old"});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "name"
        start_edit(&mut state);

        match &state.edit_mode {
            EditMode::TextPrompt { buffer, .. } => assert_eq!(buffer, "old"),
            _ => panic!("Expected TextPrompt"),
        }

        if let EditMode::TextPrompt { buffer, .. } = &mut state.edit_mode {
            *buffer = "new".to_string();
        }

        apply_edit(&mut state);

        assert_eq!(state.active_value()["name"], "new");
        assert_eq!(state.edit_mode, EditMode::Normal);
    }

    #[test]
    fn test_start_edit_cleared() {
        let data = json!({"name": "old", "active": true});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "name"
        start_edit_cleared(&mut state);

        match &state.edit_mode {
            EditMode::TextPrompt { buffer, .. } => assert_eq!(buffer, ""),
            _ => panic!("Expected TextPrompt with empty buffer"),
        }

        // Test with Boolean
        state.selected = 2; // "active"
        start_edit_cleared(&mut state);

        match &state.edit_mode {
            EditMode::TextPrompt { buffer, .. } => assert_eq!(buffer, ""),
            _ => panic!("Expected TextPrompt with empty buffer for boolean"),
        }
    }

    #[test]
    fn test_string_remains_string_even_if_numeric() {
        let data = json!({"version": "1.0"});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "version"
        start_edit(&mut state);

        if let EditMode::TextPrompt { buffer, .. } = &state.edit_mode {
            assert_eq!(buffer, "\"1.0\"");
        } else {
            panic!("Expected TextPrompt");
        }

        if let EditMode::TextPrompt { buffer, .. } = &mut state.edit_mode {
            *buffer = "\"2.0\"".to_string();
        }

        apply_edit(&mut state);

        assert!(
            state.active_value()["version"].is_string(),
            "Should remain a string if quoted"
        );
        assert_eq!(state.active_value()["version"], "2.0");

        start_edit(&mut state);
        if let EditMode::TextPrompt { buffer, .. } = &mut state.edit_mode {
            *buffer = "3.0".to_string();
        }
        apply_edit(&mut state);
        assert!(
            state.active_value()["version"].is_number(),
            "Should convert to number if unquoted"
        );
        assert_eq!(state.active_value()["version"], json!(3.0));
    }

    #[test]
    fn test_number_remains_number_if_valid() {
        let data = json!({"count": 10});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "count"
        start_edit(&mut state);

        if let EditMode::TextPrompt { buffer, .. } = &mut state.edit_mode {
            *buffer = "20".to_string();
        }

        apply_edit(&mut state);

        assert!(
            state.active_value()["count"].is_number(),
            "Should remain a number"
        );
        assert_eq!(state.active_value()["count"], 20);
    }

    #[test]
    fn test_to_json_pointer_conversion() {
        let path = vec!["services".to_string(), "web".to_string(), "0".to_string()];
        assert_eq!(to_json_pointer(&path), "/services/web/0");
    }

    #[test]
    fn test_find_sub_schema_properties() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });
        let path = vec!["name".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_find_sub_schema_items() {
        let schema = serde_json::json!({
            "type": "array",
            "items": { "type": "integer" }
        });
        let path = vec!["0".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("integer")
        );
    }

    #[test]
    fn test_find_sub_schema_nested() {
        let schema = serde_json::json!({
            "properties": {
                "a": {
                    "properties": {
                        "b": { "type": "boolean" }
                    }
                }
            }
        });
        let path = vec!["a".to_string(), "b".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("boolean")
        );
    }

    #[test]
    fn test_apply_edit_enum() {
        let data = serde_json::json!({ "level": "info" });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.selected = 1; // "level" node
        state.edit_mode = EditMode::Dropdown {
            options: vec!["debug".to_string(), "info".to_string(), "warn".to_string()],
            descriptions: vec![None, None, None],
            selected: 2, // "warn"
            scroll_offset: 0,
            filter_buffer: String::new(),
            filtered_indices: vec![0, 1, 2],
        };

        apply_edit(&mut state);

        assert_eq!(state.active_value()["level"], "warn");
        assert_eq!(state.edit_mode, EditMode::Normal);
    }

    #[test]
    fn test_find_sub_schema_complex() {
        let schema = serde_json::json!({
            "definitions": {
                "address": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    }
                }
            },
            "properties": {
                "user": {
                    "anyOf": [
                        { "$ref": "#/definitions/address" },
                        { "type": "string" }
                    ]
                }
            }
        });

        // Test resolving through anyOf and $ref
        let path = vec!["user".to_string(), "city".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_get_completions_enum() {
        let schema = serde_json::json!({
            "properties": {
                "level": {
                    "enum": ["info", "warn", "error"]
                }
            }
        });
        let mut state = EditorState::new(
            serde_json::json!({}),
            crate::format::Format::Json,
            None,
            None,
        );
        state.schema = Some(schema);

        let completions = state.completions_for_path(&["level".to_string()]);
        assert_eq!(completions.len(), 3);
        assert_eq!(completions[0].label, "info");
        assert_eq!(completions[0].kind, CompletionKind::Enum);
    }

    #[test]
    fn test_apply_edit_schema_inference() {
        let schema = serde_json::json!({
            "properties": {
                "version": { "type": "string" }
            }
        });
        // Start with Null to trigger the inference branch
        let data = serde_json::json!({ "version": null });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 1; // "version"

        state.edit_mode = EditMode::TextPrompt {
            buffer: "1.0".to_string(),
            cursor_pos: 3,
        };
        apply_edit(&mut state);

        // Should be string because schema says so, even if "1.0" looks like a number
        assert!(state.active_value()["version"].is_string());
        assert_eq!(state.active_value()["version"], "1.0");
    }

    #[test]
    fn test_get_addable_keys() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "number" },
                "c": { "type": "boolean" }
            }
        });
        let data = serde_json::json!({ "a": "existing" });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);

        let addable = get_addable_keys_with_descriptions(&state, &[]);
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["b", "c"]);
    }

    #[test]
    fn test_resolve_schema_type_and_default() {
        let schema = serde_json::json!({
            "properties": {
                "ports": {
                    "type": ["array", "null"],
                    "items": { "type": "string" }
                },
                "env": {
                    "anyOf": [
                        { "type": "object" },
                        { "type": "null" }
                    ]
                }
            }
        });

        // Test array type resolution with null union
        let ports_val = get_default_value_from_schema(&schema, &["ports".to_string()]);
        assert_eq!(ports_val, serde_json::json!([]));

        // Test object type resolution inside anyOf
        let env_val = get_default_value_from_schema(&schema, &["env".to_string()]);
        assert_eq!(env_val, serde_json::json!({}));
    }

    #[test]
    fn test_boolean_value_toggled_even_with_schema_enum() {
        let schema = serde_json::json!({
            "properties": {
                "active": {
                    "type": "boolean",
                    "enum": [true, false]
                }
            }
        });
        let data = serde_json::json!({ "active": true });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 1; // "active"

        start_edit(&mut state);

        // It should toggle straight to false, rather than entering EditMode::Dropdown
        assert_eq!(state.active_value()["active"], false);
        assert_eq!(state.edit_mode, EditMode::Normal);
    }

    #[test]
    fn test_edit_value_to_array_and_object() {
        use serde_json::json;
        let data = json!({ "a": null, "b": "string" });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Edit "a" (null) to "[]"
        state.selected = 1; // "a"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "[]".to_string(),
            cursor_pos: 2,
        };
        apply_edit(&mut state);
        assert!(
            state.active_value()["a"].is_array(),
            "Should be array but was {:?}",
            state.active_value()["a"]
        );
        assert_eq!(state.active_value()["a"], json!([]));

        // Edit "b" (string) to "{}"
        state.selected = 2; // "b"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "{}".to_string(),
            cursor_pos: 2,
        };
        apply_edit(&mut state);
        assert!(
            state.active_value()["b"].is_object(),
            "Should be object but was {:?}",
            state.active_value()["b"]
        );
        assert_eq!(state.active_value()["b"], json!({}));
    }

    #[test]
    fn test_edit_string_target_preserves_string() {
        use serde_json::json;
        let data = json!({ "a": "true", "b": "some string" });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Edit "a" ("true" string) to "\"false\"" -> stays string "false"
        state.selected = 1; // "a"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "\"false\"".to_string(),
            cursor_pos: 7,
        };
        apply_edit(&mut state);
        assert!(state.active_value()["a"].is_string());
        assert_eq!(state.active_value()["a"], json!("false"));

        // Edit "b" to "\"true\"" -> stays string "true"
        state.selected = 2; // "b"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "\"true\"".to_string(),
            cursor_pos: 6,
        };
        apply_edit(&mut state);
        assert!(state.active_value()["b"].is_string());
        assert_eq!(state.active_value()["b"], json!("true"));

        // Edit "a" (now string "false") to "true" (unquoted) -> becomes boolean true
        state.selected = 1;
        start_edit(&mut state);
        state.edit_mode = EditMode::TextPrompt {
            buffer: "true".to_string(),
            cursor_pos: 4,
        };
        apply_edit(&mut state);
        assert!(state.active_value()["a"].is_boolean());
        assert_eq!(state.active_value()["a"], json!(true));
    }

    #[test]
    fn test_edit_with_explicit_quotes() {
        use serde_json::json;
        let data = json!({ "a": true });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);

        // Edit boolean "a" (true) to explicit string ""false""
        state.selected = 1; // "a"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "\"false\"".to_string(),
            cursor_pos: 7,
        };
        apply_edit(&mut state);
        assert!(state.active_value()["a"].is_string());
        assert_eq!(state.active_value()["a"], json!("false"));
    }

    #[test]
    fn test_matches_pattern_char_class() {
        assert!(matches_pattern("^[a-zA-Z0-9._-]+$", "my-service"));
        assert!(matches_pattern("^[a-zA-Z0-9._-]+$", "web_1"));
        assert!(matches_pattern("^[a-zA-Z0-9._-]+$", "app.io"));
        assert!(!matches_pattern("^[a-zA-Z0-9._-]+$", "my service"));
        assert!(!matches_pattern("^[a-zA-Z0-9._-]+$", ""));
    }

    #[test]
    fn test_matches_pattern_prefix() {
        assert!(matches_pattern("^x-", "x-custom"));
        assert!(matches_pattern("^x-", "x-"));
        assert!(!matches_pattern("^x-", "custom"));
    }

    #[test]
    fn test_matches_pattern_wildcard() {
        assert!(matches_pattern("^.+$", "anything"));
        assert!(!matches_pattern("^.+$", ""));
        assert!(matches_pattern("*", "everything"));
    }

    #[test]
    fn test_find_sub_schema_pattern_properties() {
        let schema = serde_json::json!({
            "type": "object",
            "patternProperties": {
                "^[a-zA-Z0-9._-]+$": {
                    "$ref": "#/$defs/service"
                }
            },
            "$defs": {
                "service": {
                    "type": "object",
                    "properties": {
                        "image": { "type": "string" },
                        "ports": { "type": "array" }
                    }
                }
            }
        });

        // "myservice" matches ^[a-zA-Z0-9._-]+$, should resolve to service def
        let path = vec!["myservice".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(sub.is_some(), "patternProperties should match 'myservice'");
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_find_sub_schema_compose_services_pattern() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "patternProperties": {
                        "^[a-zA-Z0-9._-]+$": { "$ref": "#/$defs/service" }
                    },
                    "additionalProperties": false
                }
            },
            "$defs": {
                "service": {
                    "type": "object",
                    "properties": {
                        "image": { "type": "string" },
                        "ports": { "type": "array" }
                    }
                }
            }
        });

        let path = vec!["services".to_string(), "web".to_string()];
        let sub = find_sub_schema(&schema, &path);
        assert!(
            sub.is_some(),
            "should resolve services.web via patternProperties"
        );

        let service_schema = sub.unwrap();
        assert_eq!(
            service_schema.get("type").and_then(|v| v.as_str()),
            Some("object")
        );

        // Check that image and ports are available as properties
        let props = service_schema
            .get("properties")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(props.contains_key("image"));
        assert!(props.contains_key("ports"));
    }

    #[test]
    fn test_get_addable_keys_pattern_properties() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "services": {
                    "type": "object",
                    "patternProperties": {
                        "^[a-zA-Z0-9._-]+$": { "$ref": "#/$defs/service" }
                    }
                }
            },
            "$defs": {
                "service": {
                    "type": "object",
                    "properties": {
                        "image": { "type": "string" },
                        "ports": { "type": "array" },
                        "environment": { "type": "object" }
                    }
                }
            }
        });

        let data = serde_json::json!({
            "services": {
                "web": { "image": "nginx" }
            }
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);

        let addable = get_addable_keys_with_descriptions(
            &state,
            &["services".to_string(), "web".to_string()],
        );
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec!["environment", "ports"]);
    }

    // ===== oneOf/anyOf tests =====

    #[test]
    fn test_oneof_variants_string_object() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string", "description": "Path to build context" },
                {
                    "type": "object",
                    "description": "Detailed build config",
                    "properties": {
                        "context": { "type": "string" },
                        "dockerfile": { "type": "string" }
                    }
                }
            ]
        });
        let variants = oneof_variants(&schema);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].type_str, "string");
        // Variant has description "Path to build context" → used as label
        assert!(variants[0].label.contains("Path to build"));
        assert_eq!(variants[1].type_str, "object");
        // Variant has description "Detailed build config" → used as label
        assert!(variants[1].label.contains("Detailed build"));
    }

    #[test]
    fn test_oneof_variants_dedup_shapes() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                { "type": "string", "minLength": 1 },
                { "type": "object", "properties": {} }
            ]
        });
        let variants = oneof_variants(&schema);
        assert_eq!(variants.len(), 2); // string + object, not 3
    }

    #[test]
    fn test_oneof_variants_anyof() {
        let schema = serde_json::json!({
            "anyOf": [
                { "type": "string" },
                { "type": "array" }
            ]
        });
        let variants = oneof_variants(&schema);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].type_str, "string");
        assert_eq!(variants[1].type_str, "array");
    }

    #[test]
    fn test_oneof_variants_single() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" }
            ]
        });
        let variants = oneof_variants(&schema);
        assert_eq!(variants.len(), 1);
    }

    #[test]
    fn test_oneof_variants_no_combo() {
        let schema = serde_json::json!({ "type": "string" });
        let variants = oneof_variants(&schema);
        assert!(variants.is_empty());
    }

    #[test]
    fn test_find_sub_schema_for_value_oneof_string() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": {
                        "context": { "type": "string" },
                        "dockerfile": { "type": "string" }
                    }
                }
            ]
        });
        let value = Some(&Value::String("./app".to_string()));
        let sub = find_sub_schema_for_value(&schema, &[], value);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_find_sub_schema_for_value_oneof_object() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": {
                        "context": { "type": "string" },
                        "dockerfile": { "type": "string" }
                    }
                }
            ]
        });
        let value = Some(&Value::Object(serde_json::Map::new()));
        let sub = find_sub_schema_for_value(&schema, &[], value);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_find_sub_schema_for_value_null_prefers_object() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": {
                        "context": { "type": "string" }
                    }
                }
            ]
        });
        let sub = find_sub_schema_for_value(&schema, &[], Some(&Value::Null));
        assert!(sub.is_some());
        // Should prefer object variant
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_find_sub_schema_for_value_none_prefers_object() {
        let schema = serde_json::json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": {}
                },
                { "type": "array" }
            ]
        });
        let sub = find_sub_schema_for_value(&schema, &[], None);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_find_sub_schema_for_value_single_variant_passthrough() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" }
            ]
        });
        let sub = find_sub_schema_for_value(&schema, &[], None);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("string")
        );
    }

    #[test]
    fn test_find_sub_schema_for_value_path_traversal() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        {
                            "type": "object",
                            "properties": {
                                "context": { "type": "string" },
                                "dockerfile": { "type": "string" }
                            }
                        }
                    ]
                }
            }
        });
        let value = Some(&Value::Object(serde_json::Map::new()));
        let sub = find_sub_schema_for_value(&schema, &["build".to_string()], value);
        assert!(sub.is_some());
        assert_eq!(
            sub.unwrap().get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_get_completions_oneof_object_variant() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        {
                            "type": "object",
                            "properties": {
                                "context": {
                                    "type": "string",
                                    "description": "Build context path"
                                },
                                "dockerfile": {
                                    "type": "string",
                                    "description": "Dockerfile name"
                                }
                            }
                        }
                    ]
                }
            }
        });
        // build is an object → should get object-variant properties
        let data = serde_json::json!({ "build": {} });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);

        let completions = state.completions_for_path(&["build".to_string()]);
        let labels: Vec<String> = completions.iter().map(|c| c.label.clone()).collect();
        assert!(labels.contains(&"context".to_string()));
        assert!(labels.contains(&"dockerfile".to_string()));
    }

    #[test]
    fn test_get_completions_oneof_string_value() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        {
                            "type": "object",
                            "properties": {
                                "context": { "type": "string" }
                            }
                        }
                    ]
                }
            }
        });
        let data = serde_json::json!({ "build": "./app" });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);

        let completions = state.completions_for_path(&["build".to_string()]);
        // String variant has no properties → should be empty
        assert!(completions.is_empty());
    }

    #[test]
    fn test_get_addable_keys_oneof_build() {
        let schema = serde_json::json!({
            "properties": {
                "services": {
                    "type": "object",
                    "patternProperties": {
                        "^[a-zA-Z0-9._-]+$": {
                            "type": "object",
                            "properties": {
                                "build": {
                                    "oneOf": [
                                        { "type": "string" },
                                        {
                                            "type": "object",
                                            "properties": {
                                                "context": { "type": "string" },
                                                "dockerfile": { "type": "string" },
                                                "args": { "type": ["array", "object"] },
                                                "labels": { "type": ["array", "object"] },
                                                "target": { "type": "string" },
                                                "shm_size": { "type": ["integer", "string"] }
                                            }
                                        }
                                    ]
                                },
                                "image": { "type": "string" }
                            }
                        }
                    }
                }
            }
        });
        let data = serde_json::json!({
            "services": {
                "web": { "image": "nginx" }
            }
        });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);

        let addable = get_addable_keys_with_descriptions(
            &state,
            &["services".to_string(), "web".to_string()],
        );
        let keys: Vec<String> = addable.iter().map(|(k, _)| k.clone()).collect();
        assert!(keys.contains(&"build".to_string()));
        // "image" is already present in web, so NOT in addable
    }

    #[test]
    fn test_extract_type_hint_oneof_string_value() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                { "type": "object" }
            ]
        });
        let value = Value::String("hello".to_string());
        let hint = crate::schema_util::extract_type_hint_for_value(&schema, Some(&value));
        assert_eq!(hint, " [String]");
    }

    #[test]
    fn test_extract_type_hint_oneof_object_value() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                { "type": "object" }
            ]
        });
        let value = Value::Object(serde_json::Map::new());
        let hint = crate::schema_util::extract_type_hint_for_value(&schema, Some(&value));
        assert_eq!(hint, " [Object]");
    }

    #[test]
    fn test_extract_type_hint_oneof_null_value() {
        let schema = serde_json::json!({
            "oneOf": [
                { "type": "string" },
                { "type": "object" }
            ]
        });
        let hint = crate::schema_util::extract_type_hint_for_value(&schema, Some(&Value::Null));
        assert_eq!(hint, " [Union]");
    }

    #[test]
    fn test_start_edit_oneof_opens_picker() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "object" }
                    ]
                }
            }
        });
        let data = serde_json::json!({ "build": null });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 1; // "build" node

        start_edit(&mut state);

        // Should open OneOfVariantDropdown, not TextPrompt
        match &state.edit_mode {
            EditMode::OneOfVariantDropdown {
                options,
                target_key,
                ..
            } => {
                assert_eq!(target_key, "build");
                assert_eq!(options.len(), 2);
                assert!(options.iter().any(|o| o.contains("String")));
                assert!(options.iter().any(|o| o.contains("Object")));
            }
            other => panic!("Expected OneOfVariantDropdown, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_oneof_variant_string() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "object" }
                    ]
                }
            }
        });
        let data = serde_json::json!({ "build": null });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 1;

        // Simulate: open picker, then select "String"
        state.edit_mode = EditMode::OneOfVariantDropdown {
            parent_path: vec![],
            target_key: "build".to_string(),
            options: vec![
                "String (path or URL)".to_string(),
                "Object (detailed config)".to_string(),
            ],
            descriptions: vec![None, None],
            selected: 0, // "String" selected
            scroll_offset: 0,
            filter_buffer: String::new(),
            cursor_pos: 0,
            filtered_indices: vec![0, 1],
        };

        apply_oneof_variant(&mut state);

        assert_eq!(state.active_value()["build"], "");
        assert!(matches!(state.edit_mode, EditMode::TextPrompt { .. }));
    }

    #[test]
    fn test_apply_oneof_variant_object() {
        let schema = serde_json::json!({
            "properties": {
                "build": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "object" }
                    ]
                }
            }
        });
        let data = serde_json::json!({ "build": null });
        let mut state = EditorState::new(data, crate::format::Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 1;

        state.edit_mode = EditMode::OneOfVariantDropdown {
            parent_path: vec![],
            target_key: "build".to_string(),
            options: vec![
                "String (path or URL)".to_string(),
                "Object (detailed config)".to_string(),
            ],
            descriptions: vec![None, None],
            selected: 1, // "Object" selected
            scroll_offset: 0,
            filter_buffer: String::new(),
            cursor_pos: 0,
            filtered_indices: vec![0, 1],
        };

        apply_oneof_variant(&mut state);

        assert!(state.active_value()["build"].is_object());
        assert!(
            state.active_value()["build"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert!(matches!(state.edit_mode, EditMode::Normal));
    }

    // ===== Duplicate-key guard tests =====

    #[test]
    fn test_duplicate_add_blocked_no_schema() {
        // Object {"abc": null}; trigger_add_child on root, then apply key "abc" → blocked
        let data = json!({"abc": null});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 0; // root object
        trigger_add_child(&mut state);
        // Set buffer to duplicate key "abc"
        if let EditMode::NewKeyPrompt { buffer, .. } = &mut state.edit_mode {
            buffer.push_str("abc");
        } else {
            panic!("Expected NewKeyPrompt after trigger_add_child on object without schema");
        }
        apply_edit(&mut state);
        // Verify: still Normal, one key only, status message set
        assert!(matches!(state.edit_mode, EditMode::Normal));
        let active = state.active_value();
        let obj = active.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("abc"));
        assert!(state.status_message.is_some());
        let msg = state
            .status_message
            .as_ref()
            .map(|(s, _)| s.as_str())
            .unwrap_or("");
        assert!(
            msg.contains("already exists"),
            "Status should warn about duplicate: {:?}",
            msg
        );
    }

    #[test]
    fn test_duplicate_add_blocked_with_schema() {
        // Schema allows "abc" as addable; add "abc" twice → second warns
        let schema = json!({
            "type": "object",
            "properties": {
                "abc": { "type": "string" },
                "xyz": { "type": "string" }
            }
        });
        let data = json!({"abc": "existing"});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 0; // root object
        trigger_add_child(&mut state);
        // Should enter NewKeyDropdown (schema has addable keys)
        match &mut state.edit_mode {
            EditMode::NewKeyDropdown {
                options,
                filter_buffer,
                filtered_indices,
                ..
            } => {
                // Filter to "abc" to select it
                filter_buffer.push_str("abc");
                // Rebuild filtered_indices to match "abc"
                let new_filtered: Vec<usize> = options
                    .iter()
                    .enumerate()
                    .filter(|(_, opt)| opt.to_lowercase().contains("abc"))
                    .map(|(i, _)| i)
                    .collect();
                *filtered_indices = new_filtered;
                // Apply — should be blocked because "abc" already exists
            }
            _ => panic!("Expected NewKeyDropdown with schema"),
        }
        apply_edit(&mut state);
        assert!(matches!(state.edit_mode, EditMode::Normal));
        let active = state.active_value();
        let obj = active.as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("abc"));
        assert!(state.status_message.is_some());
        let msg = state
            .status_message
            .as_ref()
            .map(|(s, _)| s.as_str())
            .unwrap_or("");
        assert!(msg.contains("already exists"));
    }

    #[test]
    fn test_unique_key_focuses_value() {
        // Empty object {}; add unique key "x" → selected on new node
        let data = json!({});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 0; // root object
        trigger_add_child(&mut state);
        // Set buffer to unique key "x"
        if let EditMode::NewKeyPrompt { buffer, .. } = &mut state.edit_mode {
            buffer.push('x');
        } else {
            panic!("Expected NewKeyPrompt for empty object without schema");
        }
        apply_edit(&mut state);
        // Verify the key "x" was added
        let active = state.active_value();
        let obj = active.as_object().unwrap();
        assert!(obj.contains_key("x"));
        // Verify selected points at the new key node
        let selected_path = &state.flattened_nodes[state.selected].path;
        assert_eq!(selected_path, &vec!["x".to_string()]);
    }

    #[test]
    fn test_boolean_key_no_auto_flip() {
        // Schema x: {type: boolean}; add "x" → value stays false, Normal mode
        let schema = json!({
            "type": "object",
            "properties": {
                "x": { "type": "boolean" }
            }
        });
        let data = json!({});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.schema = Some(schema);
        state.selected = 0; // root
        trigger_add_child(&mut state);
        // With schema having addable "x", it enters NewKeyDropdown
        match &mut state.edit_mode {
            EditMode::NewKeyDropdown {
                options,
                filtered_indices,
                ..
            } => {
                // Select "x" (it's the only option)
                let idx = options
                    .iter()
                    .position(|o| o == "x")
                    .expect("x should be in options");
                filtered_indices.clear();
                filtered_indices.push(idx);
                // Need to update selected to point to the right index in filtered_indices
            }
            other => panic!("Expected NewKeyDropdown with schema, got {:?}", other),
        }
        // Set selected to 0 (index into filtered_indices which has one entry)
        if let EditMode::NewKeyDropdown { selected, .. } = &mut state.edit_mode {
            *selected = 0;
        }
        apply_edit(&mut state);
        // Verify value is false (not auto-toggled to true)
        let active = state.active_value();
        let obj = active.as_object().unwrap();
        assert_eq!(obj.get("x"), Some(&json!(false)));
        // Verify mode is Normal (boolean → no auto-edit)
        assert!(matches!(state.edit_mode, EditMode::Normal));
    }

    #[test]
    fn test_rename_key_prompt_blocks_duplicate() {
        // Object {"a":1,"b":2}; rename a→b → warns, b unchanged
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 1; // "a"
        state.edit_mode = EditMode::RenameKeyPrompt {
            parent_path: vec![],
            original_key: "a".to_string(),
            buffer: "b".to_string(),
            cursor_pos: 1,
            value: json!(1),
        };
        apply_edit(&mut state);
        // Should be blocked
        assert!(matches!(state.edit_mode, EditMode::Normal));
        let active = state.active_value();
        let obj = active.as_object().unwrap();
        assert_eq!(obj.get("b"), Some(&json!(2)), "b should be unchanged");
        assert!(obj.contains_key("a"), "a should still exist");
        assert!(state.status_message.is_some());
        let msg = state
            .status_message
            .as_ref()
            .map(|(s, _)| s.as_str())
            .unwrap_or("");
        assert!(msg.contains("already exists"));
    }

    #[test]
    fn test_add_sibling_after_object_node() {
        let data = json!({
            "first": "1",
            "second": "2",
            "third": "3"
        });
        let mut state = EditorState::new(data, Format::Json, None, None);
        // Find position of "first"
        let first_pos = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["first".to_string()])
            .unwrap();
        state.selected = first_pos;

        trigger_add_sibling_after(&mut state);

        assert!(matches!(state.edit_mode, EditMode::NewKeyPrompt { .. }));

        // Check node order in parent object node's children
        let root_idx = state.root;
        let children = &state.nodes[root_idx].children;
        // "first" is children[0], new_key should be inserted at index 1 (between first and second)
        let inserted_node_idx = children[1];
        let inserted_node = &state.nodes[inserted_node_idx];
        assert_eq!(inserted_node.path, vec!["new_key".to_string()]);
    }

    #[test]
    fn test_add_sibling_after_array_node() {
        let data = json!([10, 20, 30]);
        let mut state = EditorState::new(data, Format::Json, None, None);
        // Select index 1 (element 20)
        let item1_pos = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == vec!["1".to_string()])
            .unwrap();
        state.selected = item1_pos;

        trigger_add_sibling_after(&mut state);

        // EditMode should start editing the newly inserted element
        let active = state.active_value();
        let arr = active.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0], 10);
        assert_eq!(arr[1], 20);
        assert_eq!(arr[2], Value::Null);
        assert_eq!(arr[3], 30);
    }

    #[test]
    fn test_add_sibling_on_root_node() {
        let data = json!({"a": 1});
        let mut state = EditorState::new(data, Format::Json, None, None);
        state.selected = 0; // Root object

        trigger_add_sibling_after(&mut state);

        // Falls back to trigger_add_child
        assert!(matches!(state.edit_mode, EditMode::NewKeyPrompt { .. }));
        if let EditMode::NewKeyPrompt { parent_path, .. } = &state.edit_mode {
            assert!(parent_path.is_empty());
        } else {
            panic!("Expected NewKeyPrompt with empty parent_path");
        }
    }
}
