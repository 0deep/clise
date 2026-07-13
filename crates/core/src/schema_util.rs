use crate::state::{CompletionItem, CompletionKind};
use serde_json::Value;

/// Detect oneOf/anyOf/allOf combo key (eliminates repeated if-else chains)
pub(crate) fn detect_combo_key(schema: &Value) -> Option<&'static str> {
    if schema.get("oneOf").is_some() {
        Some("oneOf")
    } else if schema.get("anyOf").is_some() {
        Some("anyOf")
    } else if schema.get("allOf").is_some() {
        Some("allOf")
    } else {
        None
    }
}

/// Get first non-null type from a type array (eliminates repeated find_map patterns)
pub(crate) fn first_non_null_type(types: &[Value]) -> Option<&str> {
    types
        .iter()
        .find_map(|v| v.as_str().filter(|s| *s != "null"))
}

/// Metadata for a oneOf/anyOf variant
#[derive(Debug, Clone)]
pub(crate) struct VariantMeta {
    /// Display label for this variant
    pub(crate) label: String,
    /// Description for tooltip
    pub(crate) description: Option<String>,
    /// The primary JSON type of this variant ("string", "object", "array", etc.)
    #[allow(dead_code)] // used in tests
    pub(crate) type_str: String,
}

/// Extract variant metadata from a oneOf/anyOf sub-schema.
/// Deduplicates by primary type. If variant has `title` or `description`, uses that.
/// Otherwise synthesizes from the type.
pub(crate) fn oneof_variants(sub: &Value) -> Vec<VariantMeta> {
    let combo_key = match detect_combo_key(sub) {
        Some(key) if key != "allOf" => key,
        _ => return Vec::new(),
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
pub(crate) fn resolve_primary_type(schema: &Value) -> String {
    if let Some(t) = schema.get("type") {
        if let Some(s) = t.as_str() {
            return s.to_string();
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = first_non_null_type(arr) {
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
pub(crate) fn find_sub_schema_for_value<'a>(
    root: &'a Value,
    path: &[String],
    value: Option<&Value>,
) -> Option<&'a Value> {
    let sub = find_sub_schema(root, path)?;

    // If no oneOf/anyOf, or value is None (unknown), use first-match as-is
    let combo_key = match detect_combo_key(sub) {
        Some(key) if key != "allOf" => key,
        _ => return Some(sub),
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
pub(crate) fn json_value_type(val: &Value) -> &'static str {
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
pub(crate) fn pick_variant_by_type<'a>(arr: &'a [Value], target_type: &str) -> Option<&'a Value> {
    arr.iter().find(|v| resolve_primary_type(v) == target_type)
}

/// Check if a schema's `type` field includes a given type.
/// Handles both string (`"object"`) and array (`["object", "null"]`) forms.
pub(crate) fn schema_type_includes(schema: &Value, target: &str) -> bool {
    match schema.get("type") {
        Some(Value::String(s)) => s == target,
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(target)),
        _ => false,
    }
}

/// Simple JSON Schema pattern matcher for common patterns (no regex dependency).
/// Handles: ^prefix, suffix$, ^char-class+$, ^exact$, and wildcard *.
pub(crate) fn matches_pattern(pattern: &str, value: &str) -> bool {
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
pub(crate) fn parse_char_class(pattern: &str) -> Option<std::collections::HashSet<char>> {
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

/// Resolve a $ref if present, following the chain.
/// Returns original schema if any link in the chain fails to resolve.
pub(crate) fn resolve_ref<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let mut current = schema;
    while let Some(ref_path) = current.get("$ref").and_then(|v| v.as_str()) {
        if ref_path.starts_with("#/") {
            let parts: Vec<&str> = ref_path.split('/').skip(1).collect();
            let mut ref_node = root;
            for part in parts {
                let unescaped = part.replace("~1", "/").replace("~0", "~");
                match ref_node.get(unescaped) {
                    Some(next) => ref_node = next,
                    None => return schema,
                }
            }
            current = ref_node;
        } else {
            return schema;
        }
    }
    current
}

pub fn find_sub_schema<'a>(schema: &'a Value, path: &[String]) -> Option<&'a Value> {
    find_sub_schema_recursive(schema, schema, path)
}

fn find_sub_schema_recursive<'a>(
    root: &'a Value,
    current: &'a Value,
    path: &[String],
) -> Option<&'a Value> {
    // If schema has $ref and resolution fails, return None (matches old behavior)
    let current = if current.get("$ref").is_some() {
        let resolved = resolve_ref(root, current);
        if resolved == current {
            return None;
        }
        resolved
    } else {
        current
    };

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

fn collect_properties_from_schema(
    schema: &Value,
    completions: &mut Vec<CompletionItem>,
    check_duplicates: bool,
) {
    let Some(props) = schema.get("properties").and_then(|v| v.as_object()) else {
        return;
    };
    for (key, prop_schema) in props {
        if check_duplicates && completions.iter().any(|c| c.label == *key) {
            continue;
        }
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

/// Collect property completions from a schema, recursing into oneOf/anyOf object variants.
pub(crate) fn collect_property_completions(
    root: &Value,
    sub_schema: &Value,
    completions: &mut Vec<CompletionItem>,
) {
    let resolved = resolve_ref(root, sub_schema);

    // Direct object with properties
    if schema_type_includes(resolved, "object") {
        collect_properties_from_schema(resolved, completions, false);
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
                    collect_properties_from_schema(variant_resolved, completions, true);
                    collect_pattern_properties_completions(root, variant_resolved, completions);
                }
            }
        }
    }
}

/// Collect completions from patternProperties (e.g., compose's `networks`, `volumes`).
/// Each pattern's schema may be a $ref that needs resolving.
pub(crate) fn collect_pattern_properties_completions(
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

pub(crate) fn resolve_schema_type_and_default(
    root: &Value,
    current: &Value,
) -> (Option<Value>, Option<String>) {
    let current = resolve_ref(root, current);

    if let Some(def) = current.get("default") {
        return (Some(def.clone()), None);
    }

    if let Some(t_val) = current.get("type") {
        let t_str = if let Some(s) = t_val.as_str() {
            Some(s.to_string())
        } else if let Some(arr) = t_val.as_array() {
            first_non_null_type(arr).map(|s| s.to_string())
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

/// Collect addable keys from a schema, recursing into oneOf/anyOf/allOf object variants.
/// NOTE: patternProperties are NOT included — they define value schemas for
/// dynamic keys (e.g., compose's `networks`, `volumes`), not fixed key names.
pub(crate) fn collect_addable_keys(
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

// ── Schema display helpers (moved from render.rs) ──────────────────────

/// Value-aware type hint extraction.
/// When value is provided and sub_schema has oneOf/anyOf, shows the matching variant's type.
pub fn extract_type_hint_for_value(sub_schema: &Value, value: Option<&Value>) -> String {
    if sub_schema.get("enum").is_some() {
        return " [Enum]".to_string();
    }

    if let Some(t) = sub_schema.get("type") {
        if let Some(s) = t.as_str() {
            return format_type_name(s);
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = first_non_null_type(arr) {
                return format_type_name(first);
            }
        }
    }

    // Handle anyOf/oneOf/allOf
    if let Some(key) = detect_combo_key(sub_schema) {
        if let Some(arr) = sub_schema.get(key).and_then(|v| v.as_array()) {
            // If value is non-null, try to find the matching variant
            if let Some(val) = value {
                if !val.is_null() {
                    let target_type = json_value_type(val);
                    for variant in arr {
                        let hint = extract_type_hint_for_value(variant, Some(val));
                        if !hint.is_empty() {
                            // Check if this variant matches the target type
                            if let Some(t) = variant.get("type") {
                                if t.as_str() == Some(target_type) {
                                    return hint;
                                }
                            }
                        }
                    }
                }
            }
            // Value is null or no match found: show [Union] for multi-variant
            if arr.len() > 1 {
                return " [Union]".to_string();
            }
            // Single variant: recurse
            if let Some(variant) = arr.first() {
                return extract_type_hint_for_value(variant, value);
            }
        }
    }

    "".to_string()
}

/// Format type name for display
pub(crate) fn format_type_name(t: &str) -> String {
    match t {
        "string" => " [String]",
        "number" | "integer" => " [Number]",
        "boolean" => " [Bool]",
        "object" => " [Object]",
        "array" => " [Array]",
        "null" => " [Null]",
        _ => "",
    }
    .to_string()
}

/// Extract description from schema
pub fn extract_description(sub_schema: &Value) -> Option<String> {
    sub_schema
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Get placeholder text for schema type
pub(crate) fn format_type_placeholder(sub_schema: &Value) -> Option<String> {
    if sub_schema.get("enum").is_some() {
        return Some("(enum)".to_string());
    }
    if let Some(t) = sub_schema.get("type") {
        if let Some(s) = t.as_str() {
            return Some(format!("({})", s));
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = first_non_null_type(arr) {
                return Some(format!("({})", first));
            }
        }
    }
    for key in &["oneOf", "anyOf"] {
        if let Some(arr) = sub_schema.get(*key).and_then(|v| v.as_array()) {
            if arr.len() > 1 {
                return Some("(union)".to_string());
            } else if let Some(variant) = arr.first() {
                return format_type_placeholder(variant);
            }
        }
    }
    None
}
