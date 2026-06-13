use crate::state::{
    CompletionItem, CompletionKind, EditMode, EditorState, NodeType, to_json_pointer,
};
use serde_json::Value;

pub fn start_edit(state: &mut EditorState) {
    start_edit_impl(state, false);
}

pub fn start_edit_cleared(state: &mut EditorState) {
    start_edit_impl(state, true);
}
fn is_ambiguous_string(s: &str) -> bool {
    let s_lower = s.to_lowercase();
    s_lower == "true"
        || s_lower == "false"
        || s_lower == "null"
        || s_lower == "~"
        || s.parse::<i64>().is_ok()
        || s.parse::<u64>().is_ok()
        || s.parse::<f64>().is_ok()
}

fn start_edit_impl(state: &mut EditorState, clear_value: bool) {
    let (path, node_type) = match state.selected_node() {
        Some(n) => (n.path.clone(), n.node_type.clone()),
        None => return,
    };

    let pointer = to_json_pointer(&path);
    let current_value = state.data.pointer(&pointer).unwrap_or(&Value::Null);

    // Special handling for Boolean
    let is_bool = current_value.as_bool();
    if let Some(b) = is_bool {
        if !clear_value {
            state.save_to_undo();
            let new_value = Value::Bool(!b);
            if let Some(v) = state.data.pointer_mut(&pointer) {
                *v = new_value;
            }
            state.edit_mode = EditMode::Normal;
            state.rebuild_flattened();
            return;
        }
    }

    // Check schema for enum
    if let Some(schema) = &state.schema {
        if let Some(sub_schema) = find_sub_schema(schema, &path) {
            if let Some(enum_values) = sub_schema.get("enum").and_then(|v| v.as_array()) {
                let options: Vec<String> = enum_values
                    .iter()
                    .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
                    .collect();

                let selected = options
                    .iter()
                    .position(|opt| {
                        opt == &current_value.as_str().unwrap_or(&current_value.to_string())
                    })
                    .unwrap_or(0);

                state.edit_mode = EditMode::Dropdown { options, selected };
                return;
            }
        }
    }

    // For other types, enter edit mode
    match &node_type {
        NodeType::Leaf => {
            let buffer = if clear_value {
                "".to_string()
            } else {
                match current_value {
                    Value::String(s) => {
                        if is_ambiguous_string(s) {
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
                let parent_pointer = to_json_pointer(&parent_path);
                let is_parent_object = if parent_path.is_empty() {
                    state.data.is_object()
                } else {
                    state
                        .data
                        .pointer(&parent_pointer)
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

pub fn find_sub_schema<'a>(schema: &'a Value, path: &[String]) -> Option<&'a Value> {
    find_sub_schema_recursive(schema, schema, path)
}

fn find_sub_schema_recursive<'a>(
    root: &'a Value,
    current: &'a Value,
    path: &[String],
) -> Option<&'a Value> {
    let mut current = current;

    // Resolve $ref if present
    while let Some(ref_path) = current.get("$ref").and_then(|v| v.as_str()) {
        if ref_path.starts_with("#/") {
            let parts: Vec<&str> = ref_path.split('/').skip(1).collect();
            let mut ref_node = root;
            for part in parts {
                // Handle JSON Pointer escaping (~1 -> /, ~0 -> ~)
                let unescaped = part.replace("~1", "/").replace("~0", "~");
                if let Some(next) = ref_node.get(unescaped) {
                    ref_node = next;
                } else {
                    return None;
                }
            }
            current = ref_node;
        } else {
            // Non-local refs are not supported yet without schema fetcher integration
            break;
        }
    }

    if path.is_empty() {
        return Some(current);
    }

    let segment = &path[0];
    let tail = &path[1..];

    // 1. Properties
    if let Some(props) = current.get("properties") {
        if let Some(next) = props.get(segment) {
            return find_sub_schema_recursive(root, next, tail);
        }
    }

    // 2. additionalProperties
    if let Some(add_props) = current.get("additionalProperties") {
        if add_props.is_object() {
            return find_sub_schema_recursive(root, add_props, tail);
        }
    }

    // 3. Items (for array indexing)
    if let Some(items) = current.get("items") {
        // Any segment in an array path (index) maps to items schema
        if segment.parse::<usize>().is_ok() || segment == "*" {
            return find_sub_schema_recursive(root, items, tail);
        }
    }

    // 4. anyOf, oneOf, allOf
    for combo in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = current.get(combo).and_then(|v| v.as_array()) {
            for sub in arr {
                if let Some(found) = find_sub_schema_recursive(root, sub, path) {
                    return Some(found);
                }
            }
        }
    }

    None
}

pub fn get_completions_for_path(state: &EditorState, path: &[String]) -> Vec<CompletionItem> {
    let schema = match &state.schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    let sub_schema = match find_sub_schema(schema, path) {
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
    if sub_schema.get("type").and_then(|v| v.as_str()) == Some("boolean") {
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

    // 4. Property suggestions (if the path refers to an object's children we haven't reached yet)
    if sub_schema.get("type").and_then(|v| v.as_str()) == Some("object") {
        if let Some(props) = sub_schema.get("properties").and_then(|v| v.as_object()) {
            for (key, prop_schema) in props {
                completions.push(CompletionItem {
                    label: key.clone(),
                    value: prop_schema.get("default").cloned().unwrap_or(Value::Null),
                    kind: CompletionKind::Property,
                    detail: prop_schema
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
    }

    completions
}

pub fn apply_completion(state: &mut EditorState, path: &[String], item: &CompletionItem) {
    state.save_to_undo();
    let pointer = to_json_pointer(path);
    if let Some(v) = state.data.pointer_mut(&pointer) {
        *v = item.value.clone();
    }
    state.rebuild_flattened();
}

pub fn resolve_schema_type_and_default(
    root: &Value,
    current: &Value,
) -> (Option<Value>, Option<String>) {
    let mut current = current;

    // Resolve $ref if present
    while let Some(ref_path) = current.get("$ref").and_then(|v| v.as_str()) {
        if ref_path.starts_with("#/") {
            let parts: Vec<&str> = ref_path.split('/').skip(1).collect();
            let mut ref_node = root;
            let mut success = true;
            for part in parts {
                let unescaped = part.replace("~1", "/").replace("~0", "~");
                if let Some(next) = ref_node.get(unescaped) {
                    ref_node = next;
                } else {
                    success = false;
                    break;
                }
            }
            if success {
                current = ref_node;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if let Some(def) = current.get("default") {
        return (Some(def.clone()), None);
    }

    if let Some(t_val) = current.get("type") {
        let t_str = if let Some(s) = t_val.as_str() {
            Some(s.to_string())
        } else if let Some(arr) = t_val.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str())
                .find(|&s| s != "null")
                .map(|s| s.to_string())
        } else {
            None
        };
        if t_str.is_some() {
            return (None, t_str);
        }
    }

    // Try anyOf, oneOf, allOf
    for combo in ["anyOf", "oneOf", "allOf"] {
        if let Some(arr) = current.get(combo).and_then(|v| v.as_array()) {
            for sub in arr {
                let (def, t) = resolve_schema_type_and_default(root, sub);
                if def.is_some() || t.is_some() {
                    return (def, t);
                }
            }
        }
    }

    (None, None)
}

fn get_default_value_from_schema(schema: &Value, path: &[String]) -> Value {
    if let Some(sub) = find_sub_schema(schema, path) {
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

pub fn apply_edit(state: &mut EditorState) {
    match &state.edit_mode {
        EditMode::NewKeyDropdown {
            parent_path,
            temp_key,
            options,
            selected,
        } => {
            // save_to_undo has already been called in trigger_add_child
            let key = options[*selected].clone();
            let parent_path = parent_path.clone();
            let temp_key = temp_key.clone();
            state.edit_mode = EditMode::Normal;

            let parent_pointer = to_json_pointer(&parent_path);
            let val = if let Some(schema) = &state.schema {
                let mut child_path = parent_path.clone();
                child_path.push(key.clone());
                get_default_value_from_schema(schema, &child_path)
            } else {
                Value::Null
            };

            if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                if let Value::Object(map) = parent {
                    if let Some(_) = map.remove(&temp_key) {
                        map.insert(key.clone(), val);
                    }
                }
            }

            state.rebuild_flattened();

            // Move cursor to the newly changed key node
            let mut target_path = parent_path;
            target_path.push(key);
            if let Some(pos) = state
                .flattened_nodes
                .iter()
                .position(|n| n.path == target_path)
            {
                state.selected = pos;
                start_edit(state);
            }
            return;
        }
        EditMode::NewKeyPrompt {
            parent_path,
            temp_key,
            buffer,
            ..
        } => {
            let key = buffer.trim().to_string();
            let parent_path = parent_path.clone();
            let temp_key = temp_key.clone();
            state.edit_mode = EditMode::Normal;

            let parent_pointer = to_json_pointer(&parent_path);
            if key.is_empty() {
                if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                    if let Value::Object(map) = parent {
                        map.remove(&temp_key);
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
            } else {
                let val = if let Some(schema) = &state.schema {
                    let mut child_path = parent_path.clone();
                    child_path.push(key.clone());
                    get_default_value_from_schema(schema, &child_path)
                } else {
                    Value::Null
                };

                if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                    if let Value::Object(map) = parent {
                        if let Some(_) = map.remove(&temp_key) {
                            map.insert(key.clone(), val);
                        }
                    }
                }
                state.rebuild_flattened();

                // Move cursor to the newly changed key node
                let mut target_path = parent_path;
                target_path.push(key);
                if let Some(pos) = state
                    .flattened_nodes
                    .iter()
                    .position(|n| n.path == target_path)
                {
                    state.selected = pos;
                    start_edit(state);
                }
            }
            return;
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

            state.save_to_undo();
            let parent_pointer = to_json_pointer(&parent_path);

            if new_key.is_empty() {
                // Delete the key-value pair
                if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                    if let Value::Object(map) = parent {
                        map.remove(&original_key);
                    }
                }
                state.rebuild_flattened();
                // Select the parent or adjust selection
                if let Some(pos) = state
                    .flattened_nodes
                    .iter()
                    .position(|n| n.path == parent_path)
                {
                    state.selected = pos;
                }
            } else if new_key != original_key {
                // Rename the key
                if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                    if let Value::Object(map) = parent {
                        // To preserve order (approximately), we might need to rebuild the map
                        // But for now, standard remove/insert is acceptable unless specific order preservation is requested
                        // User mentioned: "(순서 보존을 위해 맵을 재구성합니다.)"
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

                state.rebuild_flattened();

                // Track key rename in state.renamed_keys
                let parent_pointer = to_json_pointer(&parent_path);
                let new_key_path = if parent_pointer.is_empty() {
                    format!("/{}", new_key)
                } else {
                    format!("{}/{}", parent_pointer, new_key)
                };
                let orig_key_path = if parent_pointer.is_empty() {
                    format!("/{}", original_key)
                } else {
                    format!("{}/{}", parent_pointer, original_key)
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

    let pointer = to_json_pointer(&path);
    state.save_to_undo();
    let original_value = state.data.pointer(&pointer);

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

            if has_quotes {
                // 명시적으로 따옴표로 감싸서 입력한 경우 따옴표를 한 겹 벗겨내고 문자열로 취급
                let unquoted = &trimmed[1..trimmed.len() - 1];
                Value::String(unquoted.to_string())
            } else if trimmed == "[]" {
                Value::Array(Vec::new())
            } else if trimmed == "{}" {
                Value::Object(serde_json::Map::new())
            } else if is_string_target {
                // 기존 값이 문자열이거나 스키마가 string을 요구하는 타겟일 때 문자열로 유지
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
        EditMode::Dropdown { options, selected } => {
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

    if let Some(v) = state.data.pointer_mut(&pointer) {
        *v = new_value;
    }

    state.edit_mode = EditMode::Normal;
    state.rebuild_flattened();
}

pub fn cancel_edit(state: &mut EditorState) {
    match &state.edit_mode {
        EditMode::NewKeyDropdown {
            parent_path,
            temp_key,
            ..
        }
        | EditMode::NewKeyPrompt {
            parent_path,
            temp_key,
            ..
        } => {
            let parent_path = parent_path.clone();
            let temp_key = temp_key.clone();
            state.edit_mode = EditMode::Normal;

            let parent_pointer = to_json_pointer(&parent_path);
            if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                if let Value::Object(map) = parent {
                    map.remove(&temp_key);
                }
            }
            state.rebuild_flattened();

            // Move cursor back to the original parent node
            if let Some(pos) = state
                .flattened_nodes
                .iter()
                .position(|n| n.path == parent_path)
            {
                state.selected = pos;
            }

            // Pop the Undo stack saved in trigger_add_child (since we manually restored data)
            state.pop_undo();
        }
        _ => {
            state.edit_mode = EditMode::Normal;
        }
    }
}
pub fn get_addable_keys(state: &EditorState, path: &[String]) -> Vec<String> {
    let schema = match &state.schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    let sub_schema = match find_sub_schema(schema, path) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut addable_keys = Vec::new();
    if sub_schema.get("type").and_then(|v| v.as_str()) == Some("object") {
        if let Some(props) = sub_schema.get("properties").and_then(|v| v.as_object()) {
            // Get current keys at this path
            let pointer = to_json_pointer(path);
            let current_keys: std::collections::HashSet<String> =
                if let Some(Value::Object(map)) = state.data.pointer(&pointer) {
                    map.keys().cloned().collect()
                } else {
                    std::collections::HashSet::new()
                };

            for (key, _) in props {
                if !current_keys.contains(key) {
                    addable_keys.push(key.clone());
                }
            }
        }
    }
    addable_keys.sort();
    addable_keys
}

pub fn trigger_add_child(state: &mut EditorState) {
    state.save_to_undo();
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
            let addable = get_addable_keys(state, &path);

            // Generate a unique temporary key (avoiding duplicates)
            let mut temp_key = "new_key".to_string();
            let parent_pointer = to_json_pointer(&path);
            if let Some(Value::Object(map)) = state.data.pointer(&parent_pointer) {
                let mut count = 0;
                while map.contains_key(&temp_key) {
                    count += 1;
                    temp_key = format!("new_key_{}", count);
                }
            }

            // Expand the parent node and insert the temporary node
            if let Some(parent_node) = state.flattened_nodes.iter_mut().find(|n| n.path == path) {
                parent_node.expanded = true;
            }

            if let Some(parent) = state.data.pointer_mut(&parent_pointer) {
                if let Value::Object(map) = parent {
                    map.insert(temp_key.clone(), Value::Null);
                }
            }

            state.rebuild_flattened();

            // Force move the cursor to the newly created temporary node
            let mut temp_path = path.clone();
            temp_path.push(temp_key.clone());
            if let Some(pos) = state
                .flattened_nodes
                .iter()
                .position(|n| n.path == temp_path)
            {
                state.selected = pos;
            }

            if !addable.is_empty() {
                state.edit_mode = EditMode::NewKeyDropdown {
                    parent_path: path,
                    temp_key,
                    options: addable,
                    selected: 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use serde_json::json;

    #[test]
    fn test_edit_boolean_toggle() {
        let data = json!({"active": true});
        let mut state = EditorState::new(data, Format::Json, None, None);

        state.selected = 1; // "active"
        start_edit(&mut state);

        assert_eq!(state.data["active"], false);
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

        assert_eq!(state.data["name"], "new");
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
            state.data["version"].is_string(),
            "Should remain a string if quoted"
        );
        assert_eq!(state.data["version"], "2.0");

        start_edit(&mut state);
        if let EditMode::TextPrompt { buffer, .. } = &mut state.edit_mode {
            *buffer = "3.0".to_string();
        }
        apply_edit(&mut state);
        assert!(
            state.data["version"].is_number(),
            "Should convert to number if unquoted"
        );
        assert_eq!(state.data["version"], json!(3.0));
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

        assert!(state.data["count"].is_number(), "Should remain a number");
        assert_eq!(state.data["count"], 20);
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
            selected: 2, // "warn"
        };

        apply_edit(&mut state);

        assert_eq!(state.data["level"], "warn");
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

        let completions = state.get_completions_for_path(&["level".to_string()]);
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
        assert!(state.data["version"].is_string());
        assert_eq!(state.data["version"], "1.0");
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

        let addable = get_addable_keys(&state, &[]);
        assert_eq!(addable, vec!["b", "c"]);
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
        assert_eq!(state.data["active"], false);
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
            state.data["a"].is_array(),
            "Should be array but was {:?}",
            state.data["a"]
        );
        assert_eq!(state.data["a"], json!([]));

        // Edit "b" (string) to "{}"
        state.selected = 2; // "b"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "{}".to_string(),
            cursor_pos: 2,
        };
        apply_edit(&mut state);
        assert!(
            state.data["b"].is_object(),
            "Should be object but was {:?}",
            state.data["b"]
        );
        assert_eq!(state.data["b"], json!({}));
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
        assert!(state.data["a"].is_string());
        assert_eq!(state.data["a"], json!("false"));

        // Edit "b" to "\"true\"" -> stays string "true"
        state.selected = 2; // "b"
        state.edit_mode = EditMode::TextPrompt {
            buffer: "\"true\"".to_string(),
            cursor_pos: 6,
        };
        apply_edit(&mut state);
        assert!(state.data["b"].is_string());
        assert_eq!(state.data["b"], json!("true"));

        // Edit "a" (now string "false") to "true" (unquoted) -> becomes boolean true
        state.selected = 1;
        start_edit(&mut state);
        state.edit_mode = EditMode::TextPrompt {
            buffer: "true".to_string(),
            cursor_pos: 4,
        };
        apply_edit(&mut state);
        assert!(state.data["a"].is_boolean());
        assert_eq!(state.data["a"], json!(true));
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
        assert!(state.data["a"].is_string());
        assert_eq!(state.data["a"], json!("false"));
    }
}
