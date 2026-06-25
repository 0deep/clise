use crate::state::{
    CompletionItem, CompletionKind, EditMode, EditorState, NodeType, to_json_pointer,
};
use serde_json::Value;

/// Metadata for a oneOf/anyOf variant
#[derive(Debug, Clone)]
pub struct VariantMeta {
    /// Display label for this variant
    pub label: String,
    /// Description for tooltip
    pub description: Option<String>,
    /// The primary JSON type of this variant ("string", "object", "array", etc.)
    pub type_str: String,
}

/// Extract variant metadata from a oneOf/anyOf sub-schema.
/// Deduplicates by primary type. If variant has `title` or `description`, uses that.
/// Otherwise synthesizes from the type.
pub fn oneof_variants(sub: &Value) -> Vec<VariantMeta> {
    let combo_key = if sub.get("oneOf").is_some() {
        "oneOf"
    } else if sub.get("anyOf").is_some() {
        "anyOf"
    } else {
        return Vec::new();
    };

    let arr = match sub.get(combo_key).and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut variants = Vec::new();
    let mut seen_types = std::collections::HashSet::new();

    for variant in arr {
        let type_str = resolve_primary_type(variant);
        if seen_types.contains(&type_str) {
            continue;
        }
        seen_types.insert(type_str.clone());

        let label = variant
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                variant
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        if s.len() > 40 {
                            format!("{}...", &s[..37])
                        } else {
                            s.to_string()
                        }
                    })
            })
            .unwrap_or_else(|| match type_str.as_str() {
                "string" => "String (path or URL)".to_string(),
                "object" => "Object (detailed config)".to_string(),
                "array" => "Array".to_string(),
                "boolean" => "Boolean".to_string(),
                "number" | "integer" => "Number".to_string(),
                "null" => "Null".to_string(),
                _ => "Value".to_string(),
            });

        let description = variant
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 80 {
                    format!("{}...", &s[..77])
                } else {
                    s.to_string()
                }
            });

        variants.push(VariantMeta {
            label,
            description,
            type_str,
        });
    }

    variants
}

/// Resolve the primary type string from a schema variant.
/// Handles direct `type`, `type` arrays (picks first non-null), and falls back to "object".
fn resolve_primary_type(schema: &Value) -> String {
    if let Some(t) = schema.get("type") {
        if let Some(s) = t.as_str() {
            return s.to_string();
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = arr.iter().find_map(|v| {
                let s = v.as_str()?;
                if s != "null" { Some(s) } else { None }
            }) {
                return first.to_string();
            }
        }
    }

    // Infer from shape: has "properties" or "patternProperties" → object
    if schema.get("properties").is_some()
        || schema.get("patternProperties").is_some()
        || schema.get("additionalProperties").is_some()
    {
        return "object".to_string();
    }

    // Infer from shape: has "items" → array
    if schema.get("items").is_some() {
        return "array".to_string();
    }

    "object".to_string()
}

/// Value-aware schema resolver for oneOf/anyOf boundaries.
/// Calls `find_sub_schema` to descend the path, then at each oneOf/anyOf,
/// filters variants by the current value's type.
pub fn find_sub_schema_for_value<'a>(
    root: &'a Value,
    path: &[String],
    value: Option<&Value>,
) -> Option<&'a Value> {
    let sub = find_sub_schema(root, path)?;

    // If no oneOf/anyOf, or value is None (unknown), use first-match as-is
    let combo_key = if sub.get("oneOf").is_some() {
        "oneOf"
    } else if sub.get("anyOf").is_some() {
        "anyOf"
    } else {
        return Some(sub);
    };

    let arr = match sub.get(combo_key).and_then(|v| v.as_array()) {
        Some(a) if a.len() > 1 => a,
        Some(a) if a.len() == 1 => {
            // Single variant: return the variant itself, not the wrapper
            return a.first();
        }
        _ => return Some(sub), // empty → no choice needed
    };

    match value {
        Some(Value::Null) | None => {
            // Value is null or unknown: prefer first object/array variant
            if let Some(v) = arr
                .iter()
                .find(|v| matches!(resolve_primary_type(v).as_str(), "object" | "array"))
            {
                Some(v)
            } else {
                arr.first()
            }
        }
        Some(val) => {
            // Non-null value: find the variant whose type matches
            let target_type = json_value_type(val);
            // Prefer exact type match
            if let Some(v) = pick_variant_by_type(arr, &target_type) {
                Some(v)
            } else {
                // Fallback: first variant (existing heuristic)
                arr.first()
            }
        }
    }
}

/// Get the JSON Schema type name for a serde_json::Value
fn json_value_type(val: &Value) -> &'static str {
    match val {
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::Bool(_) => "boolean",
        Value::Null => "null",
    }
}

/// Find the variant that best matches a target type.
/// Preference: object > array > primitives.
fn pick_variant_by_type<'a>(arr: &'a [Value], target_type: &str) -> Option<&'a Value> {
    let mut candidates: Vec<(&Value, usize)> = arr
        .iter()
        .filter_map(|v| {
            let t = resolve_primary_type(v);
            if t == target_type {
                Some((v, 0)) // exact match
            } else {
                None
            }
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Sort by priority: object(0) > array(1) > primitive(2)
    candidates.sort_by_key(|(_, _)| 0); // all same priority
    candidates.first().map(|(v, _)| *v)
}

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
    if let Some(b) = is_bool.filter(|_| !clear_value) {
        state.save_to_undo();
        let new_value = Value::Bool(!b);
        if let Some(v) = state.data.pointer_mut(&pointer) {
            *v = new_value;
        }
        state.edit_mode = EditMode::Normal;
        state.rebuild_flattened();
        return;
    }

    // Check schema for enum
    if let Some(sub_schema) = state
        .schema
        .as_ref()
        .and_then(|s| find_sub_schema(s, &path))
    {
        if let Some(enum_values) = sub_schema.get("enum").and_then(|v| v.as_array()) {
            let options: Vec<String> = enum_values
                .iter()
                .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
                .collect();

            // Read parallel enumDescriptions array if present
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
                .position(|opt| {
                    opt == &current_value.as_str().unwrap_or(&current_value.to_string())
                })
                .unwrap_or(0);

            state.edit_mode = EditMode::Dropdown {
                options: options.clone(),
                descriptions,
                selected,
                scroll_offset: 0,
                filter_buffer: String::new(),
                filtered_indices: (0..options.len()).collect(),
            };
            return;
        }
    }

    // Check for oneOf/anyOf variant picker when value is null
    if *current_value == Value::Null && !clear_value {
        if let Some(sub_schema) = state
            .schema
            .as_ref()
            .and_then(|s| find_sub_schema(s, &path))
        {
            let combo_key = if sub_schema.get("oneOf").is_some() {
                Some("oneOf")
            } else if sub_schema.get("anyOf").is_some() {
                Some("anyOf")
            } else {
                None
            };

            if let Some(key) = combo_key {
                if let Some(arr) = sub_schema.get(key).and_then(|v| v.as_array()) {
                    if arr.len() > 1 {
                        let variants = oneof_variants(sub_schema);
                        if variants.len() > 1 {
                            let parent_path = path[..path.len().saturating_sub(1)].to_vec();
                            let target_key = path.last().cloned().unwrap_or_default();
                            let options: Vec<String> =
                                variants.iter().map(|v| v.label.clone()).collect();
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
                            return;
                        }
                    }
                }
            }
        }
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
                            if let Some(v) = state.data.pointer_mut(&to_json_pointer(&path)) {
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

/// Check if a schema's `type` field includes a given type.
/// Handles both string (`"object"`) and array (`["object", "null"]`) forms.
fn schema_type_includes(schema: &Value, target: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(s)) => s == target,
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(target)),
        _ => false,
    }
}

/// Simple JSON Schema pattern matcher for common patterns (no regex dependency).
/// Handles: ^prefix, suffix$, ^char-class+$, ^exact$, and wildcard *.
fn matches_pattern(pattern: &str, value: &str) -> bool {
    // Detect anchored prefix: "^x-" (starts with ^, no trailing $)
    let is_anchored_prefix = pattern.starts_with('^') && !pattern.ends_with('$');

    let p = pattern.trim_start_matches('^').trim_end_matches('$');

    if p == ".+" || p == ".*" || p == "*" {
        return !value.is_empty() || p != ".+";
    }

    // Anchored prefix without $ → value must start with the pattern
    if is_anchored_prefix && p != "." {
        return value.starts_with(p);
    }

    // Prefix: "x-*" after stripping ^
    if let Some(prefix) = p.strip_suffix('*') {
        return value.starts_with(prefix);
    }

    // Suffix: "*.json" after stripping $
    if let Some(suffix) = p.strip_prefix('*') {
        return value.ends_with(suffix);
    }

    // Character class pattern like [a-zA-Z0-9._-]+
    if let Some(chars) = parse_char_class(p) {
        if p.ends_with('+') || p.ends_with('*') {
            return !value.is_empty() && value.chars().all(|c| chars.contains(&c));
        }
        return chars.contains(&value.chars().next().unwrap_or('\0'));
    }

    // Exact match
    value == p
}

/// Parse a simple character class like [a-zA-Z0-9._-] into a set of chars.
fn parse_char_class(pattern: &str) -> Option<std::collections::HashSet<char>> {
    let bracket_start = pattern.find('[')?;
    let bracket_end = pattern.rfind(']')?;
    if bracket_end <= bracket_start {
        return None;
    }
    let inner = &pattern[bracket_start + 1..bracket_end];
    let rest = &pattern[bracket_end + 1..];

    // Must start at position 0 and rest must be empty or a quantifier
    if bracket_start != 0 || (!rest.is_empty() && rest != "+" && rest != "*") {
        return None;
    }

    let mut chars = std::collections::HashSet::new();
    let chars_inner: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < chars_inner.len() {
        if i + 2 < chars_inner.len() && chars_inner[i + 1] == '-' {
            // Range like a-z
            let start = chars_inner[i] as u32;
            let end = chars_inner[i + 2] as u32;
            if start <= end {
                for cp in start..=end {
                    if let Some(c) = char::from_u32(cp) {
                        chars.insert(c);
                    }
                }
            }
            i += 3;
        } else {
            chars.insert(chars_inner[i]);
            i += 1;
        }
    }
    Some(chars)
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

    // 2. patternProperties (e.g. compose-spec's "services", "networks", etc.)
    if let Some(pattern_props) = current.get("patternProperties").and_then(|v| v.as_object()) {
        for (pattern, pattern_schema) in pattern_props {
            if matches_pattern(pattern, segment) {
                return find_sub_schema_recursive(root, pattern_schema, tail);
            }
        }
    }

    // 3. additionalProperties
    if let Some(add_props) = current.get("additionalProperties") {
        if add_props.is_object() {
            return find_sub_schema_recursive(root, add_props, tail);
        }
    }

    // 4. Items (for array indexing)
    if let Some(items) = current.get("items") {
        // Any segment in an array path (index) maps to items schema
        if segment.parse::<usize>().is_ok() || segment == "*" {
            return find_sub_schema_recursive(root, items, tail);
        }
    }

    // 5. anyOf, oneOf, allOf
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

    // Get the current value at this path for value-aware resolution
    let pointer = to_json_pointer(path);
    let current_value = state.data.pointer(&pointer);

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

/// Collect property completions from a schema, recursing into oneOf/anyOf object variants.
fn collect_property_completions(
    root: &Value,
    sub_schema: &Value,
    completions: &mut Vec<CompletionItem>,
) {
    let resolved = resolve_ref(root, sub_schema);

    // Direct object with properties
    if schema_type_includes(resolved, "object") {
        if let Some(props) = resolved.get("properties").and_then(|v| v.as_object()) {
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
        // Also collect from patternProperties
        collect_pattern_properties_completions(root, resolved, completions);
        return;
    }

    // For oneOf/anyOf/allOf: collect from all object-typed variants
    for combo in ["oneOf", "anyOf", "allOf"] {
        if let Some(arr) = resolved.get(combo).and_then(|v| v.as_array()) {
            for variant in arr {
                let variant_resolved = resolve_ref(root, variant);
                if schema_type_includes(variant_resolved, "object") {
                    if let Some(props) = variant_resolved
                        .get("properties")
                        .and_then(|v| v.as_object())
                    {
                        for (key, prop_schema) in props {
                            if !completions.iter().any(|c| c.label == *key) {
                                completions.push(CompletionItem {
                                    label: key.clone(),
                                    value: prop_schema
                                        .get("default")
                                        .cloned()
                                        .unwrap_or(Value::Null),
                                    kind: CompletionKind::Property,
                                    detail: prop_schema
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                });
                            }
                        }
                    }
                    collect_pattern_properties_completions(root, variant_resolved, completions);
                }
            }
        }
    }
}

/// Collect completions from patternProperties (e.g., compose's `networks`, `volumes`).
/// Each pattern's schema may be a $ref that needs resolving.
fn collect_pattern_properties_completions(
    root: &Value,
    schema: &Value,
    completions: &mut Vec<CompletionItem>,
) {
    if let Some(pattern_props) = schema.get("patternProperties").and_then(|v| v.as_object()) {
        for (_pattern, pattern_schema) in pattern_props {
            let resolved = resolve_ref(root, pattern_schema);
            if let Some(props) = resolved.get("properties").and_then(|v| v.as_object()) {
                for (key, prop_schema) in props {
                    if !completions.iter().any(|c| c.label == *key) {
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
        }
    }
}

/// Resolve a $ref if present, otherwise return as-is.
fn resolve_ref<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(ref_path) = schema.get("$ref").and_then(|v| v.as_str()) {
        if ref_path.starts_with("#/") {
            let parts: Vec<&str> = ref_path.split('/').skip(1).collect();
            let mut ref_node = root;
            for part in parts {
                let unescaped = part.replace("~1", "/").replace("~0", "~");
                if let Some(next) = ref_node.get(unescaped) {
                    ref_node = next;
                } else {
                    return schema;
                }
            }
            return ref_node;
        }
    }
    schema
}

pub fn apply_completion(state: &mut EditorState, path: &[String], item: &CompletionItem) {
    state.save_to_undo();
    let pointer = to_json_pointer(path);
    if let Some(v) = state.data.pointer_mut(&pointer) {
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
    let pointer = to_json_pointer(&full_path);
    state.save_to_undo();
    if let Some(v) = state.data.pointer_mut(&pointer) {
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
            // save_to_undo has already been called in trigger_add_child
            let key = if filtered_indices.is_empty() {
                // No match — use typed text as the key name
                filter_buffer.clone()
            } else {
                options[*selected].clone()
            };
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

            // Empty buffer + null value: generate schema-aware default
            if trimmed.is_empty() && original_value.is_some_and(|v| v.is_null()) {
                let schema_type_resolved = if let Some(schema) = &state.schema {
                    crate::edit::resolve_schema_type_and_default(
                        schema,
                        crate::edit::find_sub_schema_for_value(schema, &path, Some(&Value::Null))
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
pub fn get_addable_keys_with_descriptions(
    state: &EditorState,
    path: &[String],
) -> Vec<(String, Option<String>)> {
    let schema = match &state.schema {
        Some(s) => s,
        None => return Vec::new(),
    };

    // Get current value at this path for value-aware resolution
    let pointer = to_json_pointer(path);
    let current_value = state.data.pointer(&pointer);

    let sub_schema = match find_sub_schema_for_value(schema, path, current_value) {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut addable_keys = Vec::new();

    // Get current keys at this path
    let current_keys: std::collections::HashSet<String> =
        if let Some(Value::Object(map)) = state.data.pointer(&pointer) {
            map.keys().cloned().collect()
        } else {
            std::collections::HashSet::new()
        };

    collect_addable_keys(schema, sub_schema, &current_keys, &mut addable_keys);
    addable_keys.sort_by(|a, b| a.0.cmp(&b.0));
    addable_keys
}

/// Collect addable keys from a schema, recursing into oneOf/anyOf/allOf object variants.
/// NOTE: patternProperties are NOT included — they define value schemas for
/// dynamic keys (e.g., compose's `networks`, `volumes`), not fixed key names.
fn collect_addable_keys(
    root: &Value,
    sub_schema: &Value,
    current_keys: &std::collections::HashSet<String>,
    addable_keys: &mut Vec<(String, Option<String>)>,
) {
    let resolved = resolve_ref(root, sub_schema);

    if schema_type_includes(resolved, "object") {
        if let Some(props) = resolved.get("properties").and_then(|v| v.as_object()) {
            for (key, prop_schema) in props {
                if !current_keys.contains(key) && !addable_keys.iter().any(|(k, _)| k == key) {
                    let desc = prop_schema
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    addable_keys.push((key.clone(), desc));
                }
            }
        }
        return;
    }

    // For oneOf/anyOf/allOf: collect from all object-typed variants
    for combo in ["oneOf", "anyOf", "allOf"] {
        if let Some(arr) = resolved.get(combo).and_then(|v| v.as_array()) {
            for variant in arr {
                let variant_resolved = resolve_ref(root, variant);
                if schema_type_includes(variant_resolved, "object") {
                    if let Some(props) = variant_resolved
                        .get("properties")
                        .and_then(|v| v.as_object())
                    {
                        for (key, prop_schema) in props {
                            if !current_keys.contains(key)
                                && !addable_keys.iter().any(|(k, _)| k == key)
                            {
                                let desc = prop_schema
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                addable_keys.push((key.clone(), desc));
                            }
                        }
                    }
                }
            }
        }
    }
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
            let addable = get_addable_keys_with_descriptions(state, &path);

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
            descriptions: vec![None, None, None],
            selected: 2, // "warn"
            scroll_offset: 0,
            filter_buffer: String::new(),
            filtered_indices: vec![0, 1, 2],
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

        let completions = state.get_completions_for_path(&["build".to_string()]);
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

        let completions = state.get_completions_for_path(&["build".to_string()]);
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
        let hint = crate::render::extract_type_hint_for_value(&schema, Some(&value));
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
        let hint = crate::render::extract_type_hint_for_value(&schema, Some(&value));
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
        let hint = crate::render::extract_type_hint_for_value(&schema, Some(&Value::Null));
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

        assert_eq!(state.data["build"], "");
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

        assert!(state.data["build"].is_object());
        assert!(state.data["build"].as_object().unwrap().is_empty());
        assert!(matches!(state.edit_mode, EditMode::Normal));
    }
}
