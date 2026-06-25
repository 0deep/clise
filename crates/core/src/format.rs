use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    Json,
    Jsonc,
    Yaml,
    Toml,
}

#[derive(Error, Debug)]
pub enum FormatError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    #[error("YAML serialization error: {0}")]
    YamlSer(#[from] serde_saphyr::ser::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("TOML edit error: {0}")]
    TomlEdit(#[from] toml_edit::TomlError),
    #[error("Unknown error: {0}")]
    Message(String),
}

pub fn parse(input: &str, format: Format) -> Result<Value, FormatError> {
    match format {
        Format::Json => Ok(serde_json::from_str(input)?),
        Format::Jsonc => {
            let clean = preprocess_jsonc(input);
            Ok(serde_json::from_str(&clean)?)
        }
        Format::Yaml => Ok(serde_saphyr::from_str(input)?),
        Format::Toml => Ok(toml::from_str(input)?),
    }
}

pub fn serialize(
    value: &Value,
    format: Format,
    original_text: Option<&str>,
    key_order_changed: bool,
) -> Result<String, FormatError> {
    serialize_with_renames(
        value,
        format,
        original_text,
        key_order_changed,
        &std::collections::HashMap::new(),
    )
}

pub fn serialize_with_renames(
    value: &Value,
    format: Format,
    original_text: Option<&str>,
    key_order_changed: bool,
    renamed_keys: &std::collections::HashMap<String, String>,
) -> Result<String, FormatError> {
    match format {
        Format::Json => Ok(serde_json::to_string_pretty(value)?),
        Format::Jsonc => {
            if let Some(orig) = original_text {
                let preprocessed = preprocess_jsonc(orig);
                if serde_json::from_str::<Value>(&preprocessed).is_ok() {
                    return Ok(merge_jsonc_preserving_comments(
                        orig,
                        value,
                        key_order_changed,
                        renamed_keys,
                    ));
                }
            }
            Ok(serde_json::to_string_pretty(value)?)
        }
        Format::Yaml => {
            if let Some(orig) = original_text {
                if serde_saphyr::from_str::<Value>(orig).is_ok() {
                    return Ok(merge_yaml_preserving_comments(
                        orig,
                        value,
                        key_order_changed,
                        renamed_keys,
                    ));
                }
            }
            if let Value::Object(map) = value {
                let mut s = String::new();
                for (k, v) in map {
                    s.push_str(&format_yaml_line(0, k, v));
                }
                Ok(s)
            } else {
                Ok(serde_saphyr::to_string(value)?)
            }
        }
        Format::Toml => {
            if let Some(orig) = original_text {
                match orig.parse::<toml_edit::DocumentMut>() {
                    Ok(mut doc) => {
                        merge_document(&mut doc, value, key_order_changed, renamed_keys);
                        return Ok(doc.to_string());
                    }
                    Err(_) => {}
                }
            }
            Ok(toml::to_string_pretty(value)?)
        }
    }
}

fn merge_jsonc_preserving_comments(
    original_text: &str,
    updated_value: &Value,
    key_order_changed: bool,
    renamed_keys: &std::collections::HashMap<String, String>,
) -> String {
    let mut blocks: std::collections::HashMap<
        String,
        std::collections::HashMap<String, Vec<String>>,
    > = std::collections::HashMap::new();
    let mut original_key_orders: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut pending_comments: Vec<String> = Vec::new();
    let mut path_stack: Vec<String> = Vec::new();
    let mut array_indices: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    let lines: Vec<&str> = original_text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || (trimmed.starts_with("/*") && trimmed.ends_with("*/"))
        {
            pending_comments.push(line.to_string());
            i += 1;
            continue;
        }

        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            pending_comments.push(line.to_string());
            i += 1;
            while i < lines.len() {
                let next_line = lines[i];
                pending_comments.push(next_line.to_string());
                if next_line.contains("*/") {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        let starts_with_close = trimmed.starts_with('}') || trimmed.starts_with(']');
        if starts_with_close {
            if !path_stack.is_empty() {
                path_stack.pop();
            }
            let comment_part = find_comment_part(trimmed);
            if !comment_part.is_empty() {
                pending_comments.push(comment_part.to_string());
            }
            i += 1;
            continue;
        }

        let comment_part = find_comment_part(trimmed);
        let no_comment_trimmed = if !comment_part.is_empty() {
            trimmed[..trimmed.len() - comment_part.len()].trim()
        } else {
            trimmed
        };

        if no_comment_trimmed == "{" || no_comment_trimmed == "[" {
            let parent_ptr = crate::state::to_json_pointer(&path_stack);
            let is_in_array = updated_value
                .pointer(&parent_ptr)
                .map(|v| v.is_array())
                .unwrap_or(false);
            if is_in_array {
                let idx_ref = array_indices.entry(parent_ptr.clone()).or_insert(0);
                let idx = *idx_ref;
                *idx_ref += 1;
                let idx_str = idx.to_string();

                let key_orders = original_key_orders.entry(parent_ptr.clone()).or_default();
                if !key_orders.contains(&idx_str) {
                    key_orders.push(idx_str.clone());
                }

                let block_vec = blocks
                    .entry(parent_ptr.clone())
                    .or_default()
                    .entry(idx_str.clone())
                    .or_default();
                for comment in pending_comments.drain(..) {
                    block_vec.push(comment);
                }

                let comment_part = find_comment_part(trimmed);
                if !comment_part.is_empty() {
                    block_vec.push(format!("__inline_comment:{}", comment_part));
                }

                path_stack.push(idx_str);
            }
            i += 1;
            continue;
        }

        let mut key = String::new();
        let mut colon_pos = None;
        let mut in_str = false;
        let mut escape = false;

        let chars: Vec<char> = trimmed.chars().collect();
        let mut char_idx = 0;
        while char_idx < chars.len() {
            let c = chars[char_idx];
            if in_str {
                if escape {
                    escape = false;
                } else if c == '\\' {
                    escape = true;
                } else if c == '"' {
                    in_str = false;
                } else {
                    key.push(c);
                }
            } else {
                if c == '"' {
                    in_str = true;
                } else if c == ':' {
                    colon_pos = Some(char_idx);
                    break;
                }
            }
            char_idx += 1;
        }

        let parent_ptr = crate::state::to_json_pointer(&path_stack);

        if let Some(c_pos) = colon_pos {
            let mut key = key;
            key = find_new_key_for_original_key(&parent_ptr, &key, renamed_keys);
            let value_part = trimmed[c_pos + 1..].trim();
            let comment_part = find_comment_part(value_part);

            let key_orders = original_key_orders.entry(parent_ptr.clone()).or_default();
            if !key_orders.contains(&key) {
                key_orders.push(key.clone());
            }

            let block_vec = blocks
                .entry(parent_ptr.clone())
                .or_default()
                .entry(key.clone())
                .or_default();
            for comment in pending_comments.drain(..) {
                block_vec.push(comment);
            }

            if !comment_part.is_empty() {
                block_vec.push(format!("__inline_comment:{}", comment_part));
            }

            let val_no_comment = if !comment_part.is_empty() {
                value_part[..value_part.len() - comment_part.len()].trim()
            } else {
                value_part
            };

            let is_open_object = val_no_comment.starts_with('{');
            let is_open_array = val_no_comment.starts_with('[');

            if is_open_object || is_open_array {
                path_stack.push(key.clone());
            }
        } else {
            let idx_ref = array_indices.entry(parent_ptr.clone()).or_insert(0);
            let idx = *idx_ref;
            *idx_ref += 1;
            let idx_str = idx.to_string();

            let key_orders = original_key_orders.entry(parent_ptr.clone()).or_default();
            if !key_orders.contains(&idx_str) {
                key_orders.push(idx_str.clone());
            }

            let block_vec = blocks
                .entry(parent_ptr.clone())
                .or_default()
                .entry(idx_str.clone())
                .or_default();
            for comment in pending_comments.drain(..) {
                block_vec.push(comment);
            }

            let comment_part = find_comment_part(trimmed);
            if !comment_part.is_empty() {
                block_vec.push(format!("__inline_comment:{}", comment_part));
            }

            let val_no_comment = if !comment_part.is_empty() {
                trimmed[..trimmed.len() - comment_part.len()].trim()
            } else {
                trimmed
            };

            if val_no_comment.starts_with('{') || val_no_comment.starts_with('[') {
                path_stack.push(idx_str.clone());
            }
        }

        i += 1;
    }

    if !pending_comments.is_empty() {
        let block_vec = blocks
            .entry("".to_string())
            .or_default()
            .entry("__trailing_comments".to_string())
            .or_default();
        for comment in pending_comments.drain(..) {
            block_vec.push(comment);
        }
    }

    let mut result = assemble_jsonc_block(
        "",
        updated_value,
        &blocks,
        &original_key_orders,
        key_order_changed,
        0,
    );

    if !original_text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn assemble_jsonc_block(
    parent_ptr: &str,
    updated_value: &Value,
    blocks: &std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>>,
    original_key_orders: &std::collections::HashMap<String, Vec<String>>,
    key_order_changed: bool,
    indent: usize,
) -> String {
    let mut s = String::new();
    let current_val = if parent_ptr.is_empty() {
        Some(updated_value)
    } else {
        updated_value.pointer(parent_ptr)
    };

    let indent_str = " ".repeat(indent);

    if let Some(val) = current_val {
        match val {
            Value::Object(map) => {
                s.push_str("{\n");

                let mut visited = std::collections::HashSet::new();
                let mut output_keys = Vec::new();

                if !key_order_changed {
                    if let Some(orig_keys) = original_key_orders.get(parent_ptr) {
                        for k in orig_keys {
                            if map.contains_key(k) {
                                output_keys.push(k.clone());
                                visited.insert(k.clone());
                            }
                        }
                    }
                }

                for k in map.keys() {
                    if !visited.contains(k) {
                        output_keys.push(k.clone());
                    }
                }

                let len = output_keys.len();
                for (idx, k) in output_keys.iter().enumerate() {
                    let child_ptr = if parent_ptr.is_empty() {
                        format!("/{}", k)
                    } else {
                        format!("{}/{}", parent_ptr, k)
                    };

                    let child_indent = indent + 2;
                    let child_indent_str = " ".repeat(child_indent);

                    let mut inline_comment = String::new();
                    if let Some(block_vec) = blocks.get(parent_ptr).and_then(|m| m.get(k)) {
                        let mut first_line_indent = None;
                        for line in block_vec {
                            if !line.starts_with("__inline_comment:") {
                                let trimmed = line.trim_start();
                                if !trimmed.is_empty() {
                                    first_line_indent = Some(line.len() - trimmed.len());
                                    break;
                                }
                            }
                        }

                        for line in block_vec {
                            if line.starts_with("__inline_comment:") {
                                inline_comment = line["__inline_comment:".len()..].to_string();
                            } else {
                                let trimmed = line.trim_start();
                                if trimmed.is_empty() {
                                    s.push('\n');
                                } else {
                                    let line_indent = line.len() - trimmed.len();
                                    let extra_spaces = if let Some(first_indent) = first_line_indent
                                    {
                                        line_indent.saturating_sub(first_indent)
                                    } else {
                                        0
                                    };
                                    s.push_str(&" ".repeat(child_indent + extra_spaces));
                                    s.push_str(trimmed);
                                    s.push('\n');
                                }
                            }
                        }
                    }

                    s.push_str(&child_indent_str);
                    s.push_str(&format!("\"{}\": ", k));

                    let is_container = map
                        .get(k)
                        .map(|v| v.is_object() || v.is_array())
                        .unwrap_or(false);
                    if is_container {
                        let inner = assemble_jsonc_block(
                            &child_ptr,
                            updated_value,
                            blocks,
                            original_key_orders,
                            key_order_changed,
                            child_indent,
                        );
                        s.push_str(&inner.trim_start());
                    } else {
                        let leaf_val = map.get(k).unwrap();
                        s.push_str(&serde_json::to_string(leaf_val).unwrap_or_default());
                    }

                    let is_last = idx == len - 1;
                    if is_last {
                        if !inline_comment.is_empty() {
                            s.push_str(&format!(" {}", inline_comment));
                        }
                        s.push('\n');
                    } else {
                        if !inline_comment.is_empty() {
                            s.push_str(&format!(", {}", inline_comment));
                        } else {
                            s.push(',');
                        }
                        s.push('\n');
                    }
                }

                s.push_str(&indent_str);
                s.push('}');
            }
            Value::Array(arr) => {
                s.push_str("[\n");

                let len = arr.len();
                for (idx, v) in arr.iter().enumerate() {
                    let child_ptr = format!("{}/{}", parent_ptr, idx);
                    let idx_str = idx.to_string();
                    let child_indent = indent + 2;
                    let child_indent_str = " ".repeat(child_indent);

                    let mut inline_comment = String::new();
                    if let Some(block_vec) = blocks.get(parent_ptr).and_then(|m| m.get(&idx_str)) {
                        let mut first_line_indent = None;
                        for line in block_vec {
                            if !line.starts_with("__inline_comment:") {
                                let trimmed = line.trim_start();
                                if !trimmed.is_empty() {
                                    first_line_indent = Some(line.len() - trimmed.len());
                                    break;
                                }
                            }
                        }

                        for line in block_vec {
                            if line.starts_with("__inline_comment:") {
                                inline_comment = line["__inline_comment:".len()..].to_string();
                            } else {
                                let trimmed = line.trim_start();
                                if trimmed.is_empty() {
                                    s.push('\n');
                                } else {
                                    let line_indent = line.len() - trimmed.len();
                                    let extra_spaces = if let Some(first_indent) = first_line_indent
                                    {
                                        line_indent.saturating_sub(first_indent)
                                    } else {
                                        0
                                    };
                                    s.push_str(&" ".repeat(child_indent + extra_spaces));
                                    s.push_str(trimmed);
                                    s.push('\n');
                                }
                            }
                        }
                    }

                    s.push_str(&child_indent_str);
                    if v.is_object() || v.is_array() {
                        let inner = assemble_jsonc_block(
                            &child_ptr,
                            updated_value,
                            blocks,
                            original_key_orders,
                            key_order_changed,
                            child_indent,
                        );
                        s.push_str(&inner.trim_start());
                    } else {
                        s.push_str(&serde_json::to_string(v).unwrap_or_default());
                    }

                    let is_last = idx == len - 1;
                    if is_last {
                        if !inline_comment.is_empty() {
                            s.push_str(&format!(" {}", inline_comment));
                        }
                        s.push('\n');
                    } else {
                        if !inline_comment.is_empty() {
                            s.push_str(&format!(", {}", inline_comment));
                        } else {
                            s.push(',');
                        }
                        s.push('\n');
                    }
                }

                s.push_str(&indent_str);
                s.push(']');
            }
            _ => {
                s.push_str(&serde_json::to_string(val).unwrap_or_default());
            }
        }
    }

    if parent_ptr.is_empty() {
        if let Some(block_vec) = blocks.get("").and_then(|m| m.get("__trailing_comments")) {
            for line in block_vec {
                s.push('\n');
                s.push_str(line.trim_end());
            }
        }
        s.push('\n');
    }

    s
}

fn is_valid_scheme(prefix: &str) -> bool {
    if !prefix.ends_with(':') {
        return false;
    }
    let s = &prefix[..prefix.len() - 1];
    if s.is_empty() {
        return false;
    }
    let last_valid_part = s
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'))
        .last()
        .unwrap_or("");
    if last_valid_part.is_empty() {
        return false;
    }
    let first = last_valid_part.chars().next().unwrap();
    first.is_ascii_alphabetic()
}

fn find_comment_part(val_part: &str) -> &str {
    let mut in_dbl_quote = false;
    let mut in_sgl_quote = false;
    let mut escape = false;
    let chars: Vec<(usize, char)> = val_part.char_indices().collect();

    let mut idx = 0;
    while idx < chars.len() {
        let (pos, c) = chars[idx];
        if escape {
            escape = false;
            idx += 1;
            continue;
        }
        if c == '\\' {
            escape = true;
            idx += 1;
            continue;
        }
        if in_dbl_quote {
            if c == '"' {
                in_dbl_quote = false;
            }
        } else if in_sgl_quote {
            if c == '\'' {
                in_sgl_quote = false;
            }
        } else {
            if c == '"' {
                in_dbl_quote = true;
            } else if c == '\'' {
                in_sgl_quote = true;
            } else if c == '#' {
                return &val_part[pos..];
            } else if c == '/' && idx + 1 < chars.len() && chars[idx + 1].1 == '/' {
                let prefix = &val_part[..pos];
                if !is_valid_scheme(prefix) {
                    return &val_part[pos..];
                }
            }
        }
        idx += 1;
    }
    ""
}

fn find_new_key_for_original_key(
    parent_ptr: &str,
    original_key: &str,
    renamed_keys: &std::collections::HashMap<String, String>,
) -> String {
    for (new_path, orig) in renamed_keys {
        if orig == original_key {
            let expected_prefix = if parent_ptr.is_empty() {
                "/".to_string()
            } else {
                format!("{}/", parent_ptr)
            };
            if new_path.starts_with(&expected_prefix) {
                let new_key = &new_path[expected_prefix.len()..];
                if !new_key.contains('/') {
                    return new_key.to_string();
                }
            }
        }
    }
    original_key.to_string()
}

fn merge_yaml_preserving_comments(
    original_text: &str,
    updated_value: &Value,
    key_order_changed: bool,
    renamed_keys: &std::collections::HashMap<String, String>,
) -> String {
    let mut blocks: std::collections::HashMap<
        String,
        std::collections::HashMap<String, Vec<String>>,
    > = std::collections::HashMap::new();
    let mut original_key_orders: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut pending_comments: Vec<String> = Vec::new();
    let mut path_stack: Vec<(usize, String)> = Vec::new();
    let mut array_indices: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut active_target: Option<(String, String)> = None;
    let mut inline_keys: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let lines: Vec<&str> = original_text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            pending_comments.push(line.to_string());
            i += 1;
            continue;
        }

        while let Some(&(stack_indent, _)) = path_stack.last() {
            if stack_indent >= indent {
                path_stack.pop();
            } else {
                break;
            }
        }

        let is_array_item = trimmed.starts_with("- ") || trimmed == "-";

        if is_array_item {
            let parent_path: Vec<String> = path_stack.iter().map(|(_, k)| k.clone()).collect();
            let parent_ptr = crate::state::to_json_pointer(&parent_path);

            let idx_ref = array_indices.entry(parent_ptr.clone()).or_insert(0);
            let idx = *idx_ref;
            *idx_ref += 1;
            let idx_str = idx.to_string();

            let key_orders = original_key_orders.entry(parent_ptr.clone()).or_default();
            if !key_orders.contains(&idx_str) {
                key_orders.push(idx_str.clone());
            }

            let header_res = format!("{}-", &line[..indent]);
            let block_vec = blocks
                .entry(parent_ptr.clone())
                .or_default()
                .entry(idx_str.clone())
                .or_default();
            for comment in pending_comments.drain(..) {
                block_vec.push(comment);
            }
            block_vec.push(header_res);

            path_stack.push((indent, idx_str.clone()));

            let content_part = if trimmed.starts_with("- ") {
                &trimmed[2..]
            } else {
                ""
            }
            .trim();
            let has_colon = content_part.find(": ").or_else(|| {
                if content_part.ends_with(':') {
                    Some(content_part.len() - 1)
                } else {
                    None
                }
            });

            if let Some(c_pos) = has_colon {
                let mut key = content_part[..c_pos]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                let child_parent_ptr = format!("{}/{}", parent_ptr, idx_str);
                key = find_new_key_for_original_key(&child_parent_ptr, &key, renamed_keys);
                inline_keys.insert(child_parent_ptr.clone(), key.clone());

                let key_orders_2 = original_key_orders
                    .entry(child_parent_ptr.clone())
                    .or_default();
                if !key_orders_2.contains(&key) {
                    key_orders_2.push(key.clone());
                }

                let mut child_path = parent_path.clone();
                child_path.push(idx_str.clone());
                child_path.push(key.clone());
                let child_ptr = crate::state::to_json_pointer(&child_path);

                let value_part = content_part[c_pos + 1..].trim();
                let comment_part = find_comment_part(value_part);

                let block_vec_2 = blocks
                    .entry(child_parent_ptr.clone())
                    .or_default()
                    .entry(key.clone())
                    .or_default();
                active_target = Some((child_parent_ptr.clone(), key.clone()));

                if let Some(target_val) = updated_value.pointer(&child_ptr) {
                    let is_flow_array = value_part.starts_with('[') && target_val.is_array();
                    let is_flow_object = value_part.starts_with('{') && target_val.is_object();
                    let is_flow_container = is_flow_array || is_flow_object;
                    let is_container =
                        (target_val.is_array() || target_val.is_object()) && !is_flow_container;

                    let leading_space = " ".repeat(indent + 2);

                    if !is_container {
                        let new_val_str = if is_flow_array {
                            let items: Vec<String> = target_val
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|v| serde_json::to_string(v).unwrap_or_default())
                                .collect();
                            format!("[{}]", items.join(", "))
                        } else if is_flow_object {
                            let items: Vec<String> = target_val
                                .as_object()
                                .unwrap()
                                .iter()
                                .map(|(k, v)| {
                                    format!(
                                        "{}: {}",
                                        k,
                                        serde_json::to_string(v).unwrap_or_default()
                                    )
                                })
                                .collect();
                            format!("{{{}}}", items.join(", "))
                        } else if target_val.is_string()
                            && (value_part.starts_with('|') || value_part.starts_with('>'))
                        {
                            let indicator = value_part.split_whitespace().next().unwrap_or("|");
                            let mut s = indicator.to_string();
                            let block_leading = " ".repeat(indent + 4);
                            if let Some(val_str) = target_val.as_str() {
                                for val_line in val_str.lines() {
                                    s.push('\n');
                                    if !val_line.is_empty() {
                                        s.push_str(&block_leading);
                                        s.push_str(val_line);
                                    }
                                }
                            }
                            s
                        } else {
                            serialize_leaf_yaml(target_val)
                        };

                        let line_res = if comment_part.is_empty() {
                            format!("{}{}: {}", leading_space, key, new_val_str)
                        } else {
                            format!("{}{}: {} {}", leading_space, key, new_val_str, comment_part)
                        };
                        block_vec_2.push(line_res);

                        if value_part.starts_with('|') || value_part.starts_with('>') {
                            while i + 1 < lines.len() {
                                let next_line = lines[i + 1];
                                let next_trimmed = next_line.trim_start();
                                if next_trimmed.is_empty() {
                                    i += 1;
                                } else {
                                    let next_indent = next_line.len() - next_trimmed.len();
                                    if next_indent > indent + 2 {
                                        i += 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                        active_target = None;
                    } else {
                        let line_res = if comment_part.is_empty() {
                            format!("{}{}:", leading_space, key)
                        } else {
                            format!("{}{}: {}", leading_space, key, comment_part)
                        };
                        block_vec_2.push(line_res);
                        path_stack.push((indent + 2, key));
                    }
                } else {
                    block_vec_2.push(format!("  {}{}", &line[..indent], content_part));
                }
            } else {
                let block_vec = blocks
                    .entry(parent_ptr.clone())
                    .or_default()
                    .entry(idx_str.clone())
                    .or_default();
                block_vec.pop();

                if let Some(Value::Array(arr)) = updated_value.pointer(&parent_ptr) {
                    if idx < arr.len() {
                        let target_val = &arr[idx];
                        let val_str = serialize_leaf_yaml(target_val);
                        let val_part = if trimmed.starts_with("- ") {
                            &trimmed[2..]
                        } else {
                            ""
                        }
                        .trim();
                        let comment_part = find_comment_part(val_part);

                        let leading_space = &line[..indent];
                        let line_res = if comment_part.is_empty() {
                            format!("{}- {}", leading_space, val_str)
                        } else {
                            format!("{}- {} {}", leading_space, val_str, comment_part)
                        };
                        block_vec.push(line_res);
                    } else {
                        block_vec.push(line.to_string());
                    }
                } else {
                    block_vec.push(line.to_string());
                }
                active_target = Some((parent_ptr, idx_str));
            }

            i += 1;
            continue;
        }

        let colon_res = trimmed.find(": ").or_else(|| {
            if trimmed.ends_with(':') {
                Some(trimmed.len() - 1)
            } else {
                None
            }
        });

        if let Some(colon_pos) = colon_res {
            let mut key = trimmed[..colon_pos]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();

            let parent_path: Vec<String> = path_stack.iter().map(|(_, k)| k.clone()).collect();
            let parent_ptr = crate::state::to_json_pointer(&parent_path);

            key = find_new_key_for_original_key(&parent_ptr, &key, renamed_keys);

            let key_orders = original_key_orders.entry(parent_ptr.clone()).or_default();
            if !key_orders.contains(&key) {
                key_orders.push(key.clone());
            }

            let mut current_path = parent_path.clone();
            current_path.push(key.clone());
            let current_ptr = crate::state::to_json_pointer(&current_path);

            active_target = Some((parent_ptr.clone(), key.clone()));
            let block_vec = blocks
                .entry(parent_ptr.clone())
                .or_default()
                .entry(key.clone())
                .or_default();
            for comment in pending_comments.drain(..) {
                block_vec.push(comment);
            }

            if let Some(target_val) = updated_value.pointer(&current_ptr) {
                let value_part = trimmed[colon_pos + 1..].trim();
                let comment_part = find_comment_part(value_part);

                let is_flow_array = value_part.starts_with('[') && target_val.is_array();
                let is_flow_object = value_part.starts_with('{') && target_val.is_object();
                let is_flow_container = is_flow_array || is_flow_object;
                let is_container =
                    (target_val.is_array() || target_val.is_object()) && !is_flow_container;

                let leading_space = &line[..indent];

                if !is_container {
                    let new_val_str = if is_flow_array {
                        let items: Vec<String> = target_val
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .collect();
                        format!("[{}]", items.join(", "))
                    } else if is_flow_object {
                        let items: Vec<String> = target_val
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| {
                                format!("{}: {}", k, serde_json::to_string(v).unwrap_or_default())
                            })
                            .collect();
                        format!("{{{}}}", items.join(", "))
                    } else if target_val.is_string()
                        && (value_part.starts_with('|') || value_part.starts_with('>'))
                    {
                        let indicator = value_part.split_whitespace().next().unwrap_or("|");
                        let mut s = indicator.to_string();
                        let block_leading = " ".repeat(indent + 2);
                        if let Some(val_str) = target_val.as_str() {
                            for val_line in val_str.lines() {
                                s.push('\n');
                                if !val_line.is_empty() {
                                    s.push_str(&block_leading);
                                    s.push_str(val_line);
                                }
                            }
                        }
                        s
                    } else {
                        serialize_leaf_yaml(target_val)
                    };

                    let line_res = if comment_part.is_empty() {
                        format!("{}{}: {}", leading_space, key, new_val_str)
                    } else {
                        format!("{}{}: {} {}", leading_space, key, new_val_str, comment_part)
                    };
                    block_vec.push(line_res);

                    if value_part.starts_with('|') || value_part.starts_with('>') {
                        while i + 1 < lines.len() {
                            let next_line = lines[i + 1];
                            let next_trimmed = next_line.trim_start();
                            if next_trimmed.is_empty() {
                                i += 1;
                            } else {
                                let next_indent = next_line.len() - next_trimmed.len();
                                if next_indent > indent {
                                    i += 1;
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    active_target = None;
                } else {
                    let line_res = if comment_part.is_empty() {
                        format!("{}{}:", leading_space, key)
                    } else {
                        format!("{}{}: {}", leading_space, key, comment_part)
                    };
                    block_vec.push(line_res);
                    path_stack.push((indent, key));
                }
            } else {
                block_vec.push(line.to_string());
            }
        } else {
            if let Some((ref p_ptr, ref c_key)) = active_target {
                if let Some(block_vec) = blocks.get_mut(p_ptr).and_then(|m| m.get_mut(c_key)) {
                    for comment in pending_comments.drain(..) {
                        block_vec.push(comment);
                    }
                    block_vec.push(line.to_string());
                } else {
                    pending_comments.push(line.to_string());
                }
            } else {
                pending_comments.push(line.to_string());
            }
        }

        i += 1;
    }

    if !pending_comments.is_empty() {
        let block_vec = blocks
            .entry("".to_string())
            .or_default()
            .entry("__trailing_comments".to_string())
            .or_default();
        for comment in pending_comments.drain(..) {
            block_vec.push(comment);
        }
    }

    let mut result = assemble_block(
        "",
        updated_value,
        &blocks,
        &original_key_orders,
        key_order_changed,
        0,
        &inline_keys,
    );

    if !original_text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

fn assemble_block(
    parent_ptr: &str,
    updated_value: &Value,
    blocks: &std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>>,
    original_key_orders: &std::collections::HashMap<String, Vec<String>>,
    key_order_changed: bool,
    indent: usize,
    inline_keys: &std::collections::HashMap<String, String>,
) -> String {
    let mut s = String::new();
    let current_val = if parent_ptr.is_empty() {
        Some(updated_value)
    } else {
        updated_value.pointer(parent_ptr)
    };

    if let Some(val) = current_val {
        match val {
            Value::Object(map) => {
                let mut visited = std::collections::HashSet::new();
                let inline_key = inline_keys.get(parent_ptr).cloned();

                let mut level_order_changed = false;
                if let Some(orig_keys) = original_key_orders.get(parent_ptr) {
                    let orig_existing_keys: Vec<String> = orig_keys
                        .iter()
                        .filter(|k| map.contains_key(*k))
                        .cloned()
                        .collect();
                    let current_existing_keys: Vec<String> = map
                        .keys()
                        .filter(|k| orig_keys.contains(k))
                        .cloned()
                        .collect();
                    if orig_existing_keys != current_existing_keys {
                        level_order_changed = true;
                    }
                }

                // If neither key_order_changed nor level_order_changed, assemble in original order
                if !key_order_changed && !level_order_changed {
                    if let Some(orig_keys) = original_key_orders.get(parent_ptr) {
                        for k in orig_keys {
                            if let Some(v) = map.get(k) {
                                visited.insert(k.clone());
                                let child_ptr = if parent_ptr.is_empty() {
                                    format!("/{}", k)
                                } else {
                                    format!("{}/{}", parent_ptr, k)
                                };

                                if let Some(block_vec) =
                                    blocks.get(parent_ptr).and_then(|m| m.get(k))
                                {
                                    let is_inline = Some(k) == inline_key.as_ref();
                                    for line in block_vec {
                                        if is_inline {
                                            s.push_str(line.trim_start());
                                        } else {
                                            s.push_str(line);
                                        }
                                        s.push('\n');
                                    }
                                    if v.is_object() || v.is_array() {
                                        let is_empty_val = block_vec
                                            .last()
                                            .map(|line| line.trim().ends_with(':'))
                                            .unwrap_or(false);
                                        if is_empty_val {
                                            s.push_str(&assemble_block(
                                                &child_ptr,
                                                updated_value,
                                                blocks,
                                                original_key_orders,
                                                key_order_changed,
                                                indent + 2,
                                                inline_keys,
                                            ));
                                        }
                                    }
                                } else {
                                    s.push_str(&format_yaml_line(indent, k, v));
                                }
                            }
                        }
                    }
                }

                // Render newly added nodes or when order change is triggered
                for (k, v) in map {
                    if visited.contains(k) {
                        continue;
                    }
                    let child_ptr = if parent_ptr.is_empty() {
                        format!("/{}", k)
                    } else {
                        format!("{}/{}", parent_ptr, k)
                    };

                    if let Some(block_vec) = blocks.get(parent_ptr).and_then(|m| m.get(k)) {
                        let is_inline = Some(k) == inline_key.as_ref();
                        for line in block_vec {
                            if is_inline {
                                s.push_str(line.trim_start());
                            } else {
                                s.push_str(line);
                            }
                            s.push('\n');
                        }
                        if v.is_object() || v.is_array() {
                            let is_empty_val = block_vec
                                .last()
                                .map(|line| line.trim().ends_with(':'))
                                .unwrap_or(false);
                            if is_empty_val {
                                s.push_str(&assemble_block(
                                    &child_ptr,
                                    updated_value,
                                    blocks,
                                    original_key_orders,
                                    key_order_changed,
                                    indent + 2,
                                    inline_keys,
                                ));
                            }
                        }
                    } else {
                        s.push_str(&format_yaml_line(indent, k, v));
                    }
                }
            }
            Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let child_ptr = format!("{}/{}", parent_ptr, i);
                    let idx_str = i.to_string();

                    if let Some(block_vec) = blocks.get(parent_ptr).and_then(|m| m.get(&idx_str)) {
                        let inline_key = inline_keys.get(&child_ptr).cloned();
                        if inline_key.is_some() {
                            for (line_idx, line) in block_vec.iter().enumerate() {
                                if line_idx == block_vec.len() - 1 {
                                    s.push_str(&format!("{} ", line.trim_end()));
                                } else {
                                    s.push_str(line);
                                    s.push('\n');
                                }
                            }
                        } else {
                            for line in block_vec {
                                s.push_str(line);
                                s.push('\n');
                            }
                        }
                        if v.is_object() || v.is_array() {
                            let is_empty_val = block_vec
                                .last()
                                .map(|line| line.trim() == "-" || line.trim().ends_with('-'))
                                .unwrap_or(false);
                            if is_empty_val {
                                s.push_str(&assemble_block(
                                    &child_ptr,
                                    updated_value,
                                    blocks,
                                    original_key_orders,
                                    key_order_changed,
                                    indent + 2,
                                    inline_keys,
                                ));
                            }
                        }
                    } else {
                        s.push_str(&format_yaml_array_item(indent + 2, v));
                    }
                }
            }
            _ => {}
        }
    }

    if parent_ptr.is_empty() {
        if let Some(block_vec) = blocks.get("").and_then(|m| m.get("__trailing_comments")) {
            for line in block_vec {
                s.push_str(line);
                s.push('\n');
            }
        }
    }

    s
}

fn serialize_leaf_yaml(val: &Value) -> String {
    match val {
        Value::String(s) => {
            let s_lower = s.to_lowercase();
            let needs_quotes = s.contains(':')
                || s.starts_with('[')
                || s.starts_with('{')
                || s.starts_with('#')
                || s.starts_with("//")
                || s_lower == "true"
                || s_lower == "false"
                || s_lower == "null"
                || s_lower == "~"
                || s.parse::<i64>().is_ok()
                || s.parse::<u64>().is_ok()
                || s.parse::<f64>().is_ok();
            if needs_quotes {
                format!("\"{}\"", s)
            } else {
                s.clone()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "".to_string(),
        _ => "".to_string(),
    }
}

fn format_yaml_line(indent: usize, key: &str, val: &Value) -> String {
    let leading = " ".repeat(indent);
    match val {
        Value::Object(map) => {
            let mut s = format!("{}{}:\n", leading, key);
            for (k, v) in map {
                s.push_str(&format_yaml_line(indent + 2, k, v));
            }
            s
        }
        Value::Array(arr) => {
            let mut s = format!("{}{}:\n", leading, key);
            for v in arr {
                s.push_str(&format_yaml_array_item(indent + 4, v));
            }
            s
        }
        _ => {
            format!("{0}{1}: {2}\n", leading, key, serialize_leaf_yaml(val))
        }
    }
}

fn format_yaml_array_item(indent: usize, val: &Value) -> String {
    let leading = " ".repeat(indent);
    let sub_len = indent.saturating_sub(2);
    let prefix = &leading[..sub_len];
    match val {
        Value::Object(map) => {
            let mut s = String::new();
            for (i, (k, v)) in map.iter().enumerate() {
                if i == 0 {
                    s.push_str(&format!("{}- {}: {}\n", prefix, k, serialize_leaf_yaml(v)));
                } else {
                    s.push_str(&format!("{}{}: {}\n", leading, k, serialize_leaf_yaml(v)));
                }
            }
            s
        }
        _ => {
            format!("{}- {}\n", prefix, serialize_leaf_yaml(val))
        }
    }
}

fn json_to_toml_item(json_val: &Value) -> toml_edit::Item {
    match json_val {
        Value::Null => toml_edit::Item::None,
        Value::Bool(b) => toml_edit::value(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml_edit::value(i)
            } else if let Some(u) = n.as_u64() {
                toml_edit::value(u as i64)
            } else if let Some(f) = n.as_f64() {
                toml_edit::value(f)
            } else {
                toml_edit::Item::None
            }
        }
        Value::String(s) => toml_edit::value(s.clone()),
        Value::Array(arr) => {
            let mut toml_arr = toml_edit::Array::new();
            for v in arr {
                if let Some(v_edit) = json_to_toml_value(v) {
                    toml_arr.push(v_edit);
                }
            }
            toml_edit::Item::Value(toml_edit::Value::Array(toml_arr))
        }
        Value::Object(map) => {
            let mut toml_tbl = toml_edit::Table::new();
            for (k, v) in map {
                toml_tbl.insert(k, json_to_toml_item(v));
            }
            toml_edit::Item::Table(toml_tbl)
        }
    }
}

fn json_to_toml_value(json_val: &Value) -> Option<toml_edit::Value> {
    match json_val {
        Value::Null => None,
        Value::Bool(b) => Some(toml_edit::Value::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml_edit::Value::from(i))
            } else if let Some(u) = n.as_u64() {
                Some(toml_edit::Value::from(u as i64))
            } else if let Some(f) = n.as_f64() {
                Some(toml_edit::Value::from(f))
            } else {
                None
            }
        }
        Value::String(s) => Some(toml_edit::Value::from(s.clone())),
        Value::Array(arr) => {
            let mut toml_arr = toml_edit::Array::new();
            for v in arr {
                if let Some(v_edit) = json_to_toml_value(v) {
                    toml_arr.push(v_edit);
                }
            }
            Some(toml_edit::Value::Array(toml_arr))
        }
        Value::Object(map) => {
            let mut toml_inline = toml_edit::InlineTable::new();
            for (k, v) in map {
                if let Some(v_edit) = json_to_toml_value(v) {
                    toml_inline.insert(k, v_edit);
                }
            }
            Some(toml_edit::Value::InlineTable(toml_inline))
        }
    }
}

#[allow(dead_code)]
fn merge_json_to_toml(toml_item: &mut toml_edit::Item, json_val: &Value, key_order_changed: bool) {
    merge_json_to_toml_with_path(
        toml_item,
        json_val,
        key_order_changed,
        &std::collections::HashMap::new(),
        "",
    )
}

fn merge_json_to_toml_with_path(
    toml_item: &mut toml_edit::Item,
    json_val: &Value,
    key_order_changed: bool,
    renamed_keys: &std::collections::HashMap<String, String>,
    parent_ptr: &str,
) {
    match toml_item {
        toml_edit::Item::Table(tbl) => {
            if let Value::Object(map) = json_val {
                // Apply renamed keys inside Table to preserve decorations
                for (k, _) in map {
                    let child_ptr = if parent_ptr.is_empty() {
                        format!("/{}", k)
                    } else {
                        format!("{}/{}", parent_ptr, k)
                    };
                    if let Some(orig) = renamed_keys.get(&child_ptr) {
                        if let Some(item) = tbl.remove(orig) {
                            tbl.insert(k, item);
                        }
                    }
                }

                let toml_keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
                let json_existing_keys: Vec<String> = map
                    .keys()
                    .filter(|k| toml_keys.contains(k))
                    .cloned()
                    .collect();
                let toml_existing_keys: Vec<String> = toml_keys
                    .iter()
                    .filter(|k| map.contains_key(*k))
                    .cloned()
                    .collect();
                let order_changed = key_order_changed && (json_existing_keys != toml_existing_keys);

                if order_changed {
                    let mut backup = std::collections::HashMap::new();
                    for tk in toml_keys {
                        if let Some(item) = tbl.remove(&tk) {
                            backup.insert(tk, item);
                        }
                    }
                    for (k, v) in map {
                        let child_ptr = if parent_ptr.is_empty() {
                            format!("/{}", k)
                        } else {
                            format!("{}/{}", parent_ptr, k)
                        };
                        if let Some(mut old_item) = backup.remove(k) {
                            merge_json_to_toml_with_path(
                                &mut old_item,
                                v,
                                key_order_changed,
                                renamed_keys,
                                &child_ptr,
                            );
                            tbl.insert(k, old_item);
                        } else {
                            tbl.insert(k, json_to_toml_item(v));
                        }
                    }
                } else {
                    for tk in toml_keys {
                        if !map.contains_key(&tk) {
                            tbl.remove(&tk);
                        }
                    }
                    for (k, v) in map {
                        let child_ptr = if parent_ptr.is_empty() {
                            format!("/{}", k)
                        } else {
                            format!("{}/{}", parent_ptr, k)
                        };
                        if let Some(item) = tbl.get_mut(k) {
                            merge_json_to_toml_with_path(
                                item,
                                v,
                                key_order_changed,
                                renamed_keys,
                                &child_ptr,
                            );
                        } else {
                            tbl.insert(k, json_to_toml_item(v));
                        }
                    }
                }
                return;
            }
        }
        toml_edit::Item::Value(old_val) => {
            match (old_val, json_val) {
                (toml_edit::Value::InlineTable(tbl), Value::Object(map)) => {
                    // Apply renamed keys inside InlineTable to preserve decorations
                    for (k, _) in map {
                        let child_ptr = if parent_ptr.is_empty() {
                            format!("/{}", k)
                        } else {
                            format!("{}/{}", parent_ptr, k)
                        };
                        if let Some(orig) = renamed_keys.get(&child_ptr) {
                            if let Some(val) = tbl.remove(orig) {
                                tbl.insert(k, val);
                            }
                        }
                    }

                    let toml_keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
                    let json_existing_keys: Vec<String> = map
                        .keys()
                        .filter(|k| toml_keys.contains(k))
                        .cloned()
                        .collect();
                    let toml_existing_keys: Vec<String> = toml_keys
                        .iter()
                        .filter(|k| map.contains_key(*k))
                        .cloned()
                        .collect();
                    let order_changed =
                        key_order_changed && (json_existing_keys != toml_existing_keys);

                    if order_changed {
                        let mut backup = std::collections::HashMap::new();
                        for tk in toml_keys {
                            if let Some(val) = tbl.remove(&tk) {
                                backup.insert(tk, val);
                            }
                        }
                        for (k, v) in map {
                            let child_ptr = if parent_ptr.is_empty() {
                                format!("/{}", k)
                            } else {
                                format!("{}/{}", parent_ptr, k)
                            };
                            if let Some(old_val) = backup.remove(k) {
                                let mut temp_item = toml_edit::Item::Value(old_val);
                                merge_json_to_toml_with_path(
                                    &mut temp_item,
                                    v,
                                    key_order_changed,
                                    renamed_keys,
                                    &child_ptr,
                                );
                                if let toml_edit::Item::Value(new_val) = temp_item {
                                    tbl.insert(k, new_val);
                                }
                            } else {
                                if let Some(new_val) = json_to_toml_value(v) {
                                    tbl.insert(k, new_val);
                                }
                            }
                        }
                    } else {
                        for tk in toml_keys {
                            if !map.contains_key(&tk) {
                                tbl.remove(&tk);
                            }
                        }
                        for (k, v) in map {
                            let child_ptr = if parent_ptr.is_empty() {
                                format!("/{}", k)
                            } else {
                                format!("{}/{}", parent_ptr, k)
                            };
                            if let Some(val) = tbl.get_mut(k) {
                                let mut temp_item = toml_edit::Item::Value(val.clone());
                                merge_json_to_toml_with_path(
                                    &mut temp_item,
                                    v,
                                    key_order_changed,
                                    renamed_keys,
                                    &child_ptr,
                                );
                                if let toml_edit::Item::Value(new_val) = temp_item {
                                    *val = new_val;
                                }
                            } else {
                                if let Some(new_val) = json_to_toml_value(v) {
                                    tbl.insert(k, new_val);
                                }
                            }
                        }
                    }
                    return;
                }
                (toml_edit::Value::Array(toml_arr), Value::Array(json_arr)) => {
                    let toml_len = toml_arr.len();
                    let json_len = json_arr.len();
                    if toml_len == json_len {
                        for idx in 0..toml_len {
                            if let Some(val) = toml_arr.get_mut(idx) {
                                let mut temp_item = toml_edit::Item::Value(val.clone());
                                let child_ptr = format!("{}/{}", parent_ptr, idx);
                                merge_json_to_toml_with_path(
                                    &mut temp_item,
                                    &json_arr[idx],
                                    key_order_changed,
                                    renamed_keys,
                                    &child_ptr,
                                );
                                if let toml_edit::Item::Value(new_val) = temp_item {
                                    *val = new_val;
                                }
                            }
                        }
                    } else {
                        let mut new_arr = toml_edit::Array::new();
                        for json_v in json_arr {
                            if let Some(v_edit) = json_to_toml_value(json_v) {
                                new_arr.push(v_edit);
                            }
                        }
                        *toml_arr = new_arr;
                    }
                    return;
                }
                (val, json_v) => {
                    let decor = val.decor().clone();
                    if let Some(mut new_val) = json_to_toml_value(json_v) {
                        *new_val.decor_mut() = decor;
                        *val = new_val;
                        return;
                    }
                }
            }
        }
        _ => {}
    }
    *toml_item = json_to_toml_item(json_val);
}

fn merge_document(
    doc: &mut toml_edit::DocumentMut,
    json_val: &Value,
    key_order_changed: bool,
    renamed_keys: &std::collections::HashMap<String, String>,
) {
    if let Value::Object(map) = json_val {
        // Apply renamed keys in the top-level document first
        for (k, _) in map {
            let child_ptr = format!("/{}", k);
            if let Some(orig) = renamed_keys.get(&child_ptr) {
                if let Some(item) = doc.remove(orig) {
                    doc.insert(k, item);
                }
            }
        }

        let doc_keys: Vec<String> = doc.iter().map(|(k, _)| k.to_string()).collect();
        let json_existing_keys: Vec<String> = map
            .keys()
            .filter(|k| doc_keys.contains(k))
            .cloned()
            .collect();
        let toml_existing_keys: Vec<String> = doc_keys
            .iter()
            .filter(|k| map.contains_key(*k))
            .cloned()
            .collect();
        let order_changed = key_order_changed && (json_existing_keys != toml_existing_keys);

        if order_changed {
            let mut root_prefix = None;
            if let Some(first_key) = doc_keys.first() {
                if let Some(item) = doc.get(first_key) {
                    root_prefix = match item {
                        toml_edit::Item::Value(v) => v.decor().prefix().cloned(),
                        toml_edit::Item::Table(t) => t.decor().prefix().cloned(),
                        _ => None,
                    };
                }
            }

            let mut backup = std::collections::HashMap::new();
            for dk in doc_keys {
                if let Some(item) = doc.remove(&dk) {
                    backup.insert(dk, item);
                }
            }
            let mut inserted_keys = Vec::new();
            for (k, v) in map {
                let child_ptr = format!("/{}", k);
                if let Some(mut old_item) = backup.remove(k) {
                    merge_json_to_toml_with_path(
                        &mut old_item,
                        v,
                        key_order_changed,
                        renamed_keys,
                        &child_ptr,
                    );
                    doc.insert(k, old_item);
                } else {
                    doc.insert(k, json_to_toml_item(v));
                }
                inserted_keys.push(k.clone());
            }

            if let Some(prefix) = root_prefix {
                if let Some(new_first_key) = inserted_keys.first() {
                    if let Some(item) = doc.get_mut(new_first_key) {
                        match item {
                            toml_edit::Item::Value(v) => {
                                v.decor_mut().set_prefix(prefix);
                            }
                            toml_edit::Item::Table(t) => {
                                t.decor_mut().set_prefix(prefix);
                            }
                            _ => {}
                        }
                    }
                }
            }
        } else {
            for dk in doc_keys {
                if !map.contains_key(&dk) {
                    doc.remove(&dk);
                }
            }
            for (k, v) in map {
                let child_ptr = format!("/{}", k);
                if let Some(item) = doc.get_mut(k) {
                    merge_json_to_toml_with_path(
                        item,
                        v,
                        key_order_changed,
                        renamed_keys,
                        &child_ptr,
                    );
                } else {
                    doc.insert(k, json_to_toml_item(v));
                }
            }
        }
    }
}

pub fn preprocess_jsonc(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
        StringEscape,
        LineComment,
        BlockComment,
    }

    let mut state = State::Normal;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).cloned();

        match state {
            State::Normal => {
                if c == '"' {
                    state = State::String;
                    output.push(c);
                } else if c == '/' && next == Some('/') {
                    state = State::LineComment;
                    output.push(' ');
                    output.push(' ');
                    i += 1;
                } else if c == '/' && next == Some('*') {
                    state = State::BlockComment;
                    output.push(' ');
                    output.push(' ');
                    i += 1;
                } else {
                    output.push(c);
                }
            }
            State::String => {
                if c == '\\' {
                    state = State::StringEscape;
                    output.push(c);
                } else if c == '"' {
                    state = State::Normal;
                    output.push(c);
                } else {
                    output.push(c);
                }
            }
            State::StringEscape => {
                state = State::String;
                output.push(c);
            }
            State::LineComment => {
                if c == '\n' {
                    state = State::Normal;
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
            State::BlockComment => {
                if c == '*' && next == Some('/') {
                    state = State::Normal;
                    output.push(' ');
                    output.push(' ');
                    i += 1;
                } else if c == '\n' {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
        }
        i += 1;
    }

    // Remove trailing commas
    let mut final_chars: Vec<char> = output.chars().collect();
    let mut last_comma_idx: Option<usize> = None;
    let mut state = State::Normal;
    let mut idx = 0;

    while idx < final_chars.len() {
        let c = final_chars[idx];
        match state {
            State::Normal => {
                if c == '"' {
                    state = State::String;
                    last_comma_idx = None;
                } else if c == ',' {
                    last_comma_idx = Some(idx);
                } else if c == '}' || c == ']' {
                    if let Some(comma_idx) = last_comma_idx {
                        final_chars[comma_idx] = ' ';
                        last_comma_idx = None;
                    }
                } else if !c.is_whitespace() {
                    last_comma_idx = None;
                }
            }
            State::String => {
                if c == '\\' {
                    state = State::StringEscape;
                } else if c == '"' {
                    state = State::Normal;
                }
            }
            State::StringEscape => {
                state = State::String;
            }
            _ => {}
        }
        idx += 1;
    }

    final_chars.into_iter().collect()
}

pub fn detect(path: &str, content: &str) -> Format {
    // 1. Check explicit extension
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        return Format::Yaml;
    } else if path.ends_with(".toml") {
        return Format::Toml;
    } else if path.ends_with(".jsonc") {
        return Format::Jsonc;
    } else if path.ends_with(".json") {
        return Format::Json;
    }

    // 2. Try parsing content to determine format
    let preprocessed = preprocess_jsonc(content);
    if let Ok(val) = serde_json::from_str::<Value>(&preprocessed) {
        if val.is_object() || val.is_array() {
            if content.contains("//") || content.contains("/*") {
                return Format::Jsonc;
            }
            if path.ends_with(".json") {
                return Format::Json;
            }
            if content != preprocessed {
                return Format::Jsonc;
            }
            return Format::Json;
        }
    }

    if let Ok(val) = toml::from_str::<Value>(content) {
        if val.is_object() || val.is_array() {
            return Format::Toml;
        }
    }

    if let Ok(val) = serde_saphyr::from_str::<Value>(content) {
        if val.is_object() || val.is_array() {
            return Format::Yaml;
        }
    }

    Format::Json // Default fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_by_extension() {
        assert_eq!(detect("test.json", ""), Format::Json);
        assert_eq!(detect("test.toml", ""), Format::Toml);
        assert_eq!(detect("test.yaml", ""), Format::Yaml);
        assert_eq!(detect("test.yml", ""), Format::Yaml);
    }

    #[test]
    fn test_detect_by_content() {
        // .conf file containing JSON
        assert_eq!(detect("test.conf", "{\"a\": 1}"), Format::Json);
        // .conf file containing TOML
        assert_eq!(detect("test.conf", "a = 1\n[table]\nb = 2"), Format::Toml);
        // .conf file containing YAML
        assert_eq!(detect("test.conf", "a: 1\ntable:\n  b: 2"), Format::Yaml);
        // Unknown content fallback to Json
        assert_eq!(detect("test.conf", "not a valid format"), Format::Json);
    }

    #[test]
    fn test_toml_comment_preservation() {
        let original = r#"# Root comment
title = "My App" # App title
[database]
# DB settings
host = "localhost"
port = 5432 # Port number
"#;
        let mut value = parse(original, Format::Toml).unwrap();
        // Modify value
        if let Some(obj) = value.as_object_mut() {
            obj.insert("title".to_string(), Value::String("New App".to_string()));
            if let Some(db) = obj.get_mut("database").and_then(|v| v.as_object_mut()) {
                db.insert("host".to_string(), Value::String("127.0.0.1".to_string()));
            }
        }

        let serialized = serialize(&value, Format::Toml, Some(original), false).unwrap();

        // Assert comments and formatting are preserved
        assert!(serialized.contains("# Root comment"));
        assert!(serialized.contains("title = \"New App\" # App title"));
        assert!(serialized.contains("[database]"));
        assert!(serialized.contains("# DB settings"));
        assert!(serialized.contains("host = \"127.0.0.1\""));
        assert!(serialized.contains("port = 5432 # Port number"));
    }

    #[test]
    fn test_yaml_comment_preservation() {
        let original = r#"# System Configuration
app:
  name: clise # Application name
  # Server settings
  server:
    host: 127.0.0.1 # IP Address
    port: 8080
"#;
        let mut value = parse(original, Format::Yaml).unwrap();
        // Modify value
        if let Some(app) = value.pointer_mut("/app").and_then(|v| v.as_object_mut()) {
            app.insert("name".to_string(), Value::String("new_clise".to_string()));
        }
        if let Some(server) = value
            .pointer_mut("/app/server")
            .and_then(|v| v.as_object_mut())
        {
            server.insert("port".to_string(), Value::from(9000));
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(serialized.contains("# System Configuration"));
        assert!(serialized.contains("name: new_clise # Application name"));
        assert!(serialized.contains("# Server settings"));
        assert!(serialized.contains("host: 127.0.0.1 # IP Address"));
        assert!(serialized.contains("port: 9000"));
    }

    #[test]
    fn test_yaml_comment_preservation_structural_changes() {
        let original = r#"# System Configuration
app:
  name: clise # Application name
  # Server settings
  server:
    host: 127.0.0.1 # IP Address
    port: 8080
"#;
        let mut value = parse(original, Format::Yaml).unwrap();
        // 1. Delete "server" node
        if let Some(app) = value.pointer_mut("/app").and_then(|v| v.as_object_mut()) {
            app.remove("server");
            // 2. Add "version" key
            app.insert("version".to_string(), Value::String("2.0.0".to_string()));
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        // Assert that System Configuration and app.name comments are preserved
        assert!(serialized.contains("# System Configuration"));
        assert!(serialized.contains("name: clise # Application name"));
        // Assert that deleted server node and its comments/content are removed
        assert!(!serialized.contains("server:"));
        assert!(!serialized.contains("# Server settings"));
        assert!(!serialized.contains("host: 127.0.0.1"));
        // Assert that new key is added successfully
        assert!(serialized.contains("version: 2.0.0"));
    }

    #[test]
    fn test_yaml_array_preservation() {
        let original = r#"services:
  web:
    image: nginx
    ports:
      - "8080:80" # default HTTP
      - "8443:443" # default HTTPS
    volumes:
      - .:/usr/share/nginx/html # Mount point
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        // 1. Modify ports[0] (8080:80 -> 9090:80)
        // 2. Delete ports[1] (8443:443)
        // 3. Modify volumes[0]
        if let Some(ports) = value
            .pointer_mut("/services/web/ports")
            .and_then(|v| v.as_array_mut())
        {
            ports[0] = Value::String("9090:80".to_string());
            ports.remove(1);
        }
        if let Some(volumes) = value
            .pointer_mut("/services/web/volumes")
            .and_then(|v| v.as_array_mut())
        {
            volumes[0] = Value::String("./src:/usr/share/nginx/html".to_string());
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        // Assertions
        assert!(serialized.contains("ports:"));
        assert!(serialized.contains("- \"9090:80\" # default HTTP")); // modified value + preserved comment
        assert!(!serialized.contains("8443:443")); // deleted item
        assert!(!serialized.contains("# default HTTPS")); // deleted item comment

        assert!(serialized.contains("volumes:"));
        assert!(serialized.contains("- \"./src:/usr/share/nginx/html\" # Mount point")); // modified volume with colon
    }

    #[test]
    fn test_yaml_empty_array_addition_and_indentation() {
        let original = r#"services:
  web:
    image: nginx
    ports:
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        if let Some(web) = value
            .pointer_mut("/services/web")
            .and_then(|v| v.as_object_mut())
        {
            web.insert("ports".to_string(), serde_json::json!(["80:80"]));
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(serialized.contains("ports:"));
        assert!(serialized.contains("      - \"80:80\""));
        assert!(!serialized.contains("        - \"80:80\""));
    }

    #[test]
    fn test_yaml_empty_array_literal_addition_and_indentation() {
        let original = r#"services:
  web:
    image: nginx
    ports: []
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        if let Some(ports) = value.pointer_mut("/services/web/ports") {
            *ports = serde_json::json!(["80:80"]);
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(serialized.contains("ports: [\"80:80\"]"));
    }

    #[test]
    fn test_yaml_url_in_values_no_comment_bug() {
        let original = r#"services:
  web:
    environment:
      - S3_ENDPOINT=http://10.89.0.1:9000
      - API_URL=https://example.com/api // This is a real comment
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        if let Some(env) = value
            .pointer_mut("/services/web/environment")
            .and_then(|v| v.as_array_mut())
        {
            env[1] = Value::String("API_URL=https://example.com/v2".to_string());
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(serialized.contains("- \"S3_ENDPOINT=http://10.89.0.1:9000\""));
        assert!(!serialized.contains("- \"S3_ENDPOINT=http://10.89.0.1:9000\" //10.89.0.1:9000"));
        assert!(
            serialized.contains("- \"API_URL=https://example.com/v2\" // This is a real comment")
        );
    }

    #[test]
    fn test_yaml_custom_uri_scheme_no_comment_bug() {
        let original = r#"image: docker-image://alpine:latest
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        // Value change to trigger merge logic
        if let Some(img) = value.pointer_mut("/image") {
            *img = Value::String("docker-image://alpine:3.18".to_string());
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(serialized.contains("image: \"docker-image://alpine:3.18\""));
        // Ensure no redundant comment is appended after the quoted string
        assert!(!serialized.contains("\" //"));
    }

    #[test]
    fn test_yaml_flow_style_preservation() {
        let original = r#"services:
  web:
    entrypoint: ["uv", "run", "python3", "webtoon_scraper.py"]
    meta: {a: 1, b: 2}
"#;
        let mut value = parse(original, Format::Yaml).unwrap();

        if let Some(entrypoint) = value
            .pointer_mut("/services/web/entrypoint")
            .and_then(|v| v.as_array_mut())
        {
            entrypoint[2] = Value::String("python".to_string());
        }
        if let Some(meta) = value
            .pointer_mut("/services/web/meta")
            .and_then(|v| v.as_object_mut())
        {
            meta.insert("a".to_string(), Value::from(3));
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();

        assert!(
            serialized
                .contains("entrypoint: [\"uv\", \"run\", \"python\", \"webtoon_scraper.py\"]")
        );
        assert!(serialized.contains("meta: {a: 3, b: 2}"));
    }

    #[test]
    fn test_yaml_flow_style_preservation_with_semicolon() {
        let original = r#"services:
  web:
    command: ["nginx", "-g", "daemon off;"]
"#;
        let value = parse(original, Format::Yaml).unwrap();
        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();
        assert!(
            serialized.contains("command: [\"nginx\", \"-g\", \"daemon off;\"]"),
            "Serialized output:\n{}",
            serialized
        );
    }

    #[test]
    fn test_yaml_flow_style_preservation_with_semicolon_add_item() {
        let original = r#"web:
  command:
    - nginx
    - -g
"#;
        let mut value = parse(original, Format::Yaml).unwrap();
        if let Some(cmd) = value
            .pointer_mut("/web/command")
            .and_then(|v| v.as_array_mut())
        {
            cmd.push(Value::String("daemon off;".to_string()));
        }
        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();
        assert!(
            serialized.lines().any(|l| l == "    - daemon off;"),
            "Expected 4-space indent. Serialized output:\n{}",
            serialized
        );
        assert!(
            !serialized.lines().any(|l| l == "      - daemon off;"),
            "Incorrect 6-space indent found! Serialized output:\n{}",
            serialized
        );
    }

    #[test]
    fn test_yaml_flow_style_preservation_with_semicolon_modify() {
        let original = r#"services:
  web:
    image: nginx
    command: ["nginx", "-g", "daemon off;"]
"#;
        let mut value = parse(original, Format::Yaml).unwrap();
        if let Some(cmd) = value
            .pointer_mut("/services/web/command")
            .and_then(|v| v.as_array_mut())
        {
            cmd[2] = Value::String("daemon on;".to_string());
        }
        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();
        assert!(
            serialized.contains("command: [\"nginx\", \"-g\", \"daemon on;\"]"),
            "Serialized output:\n{}",
            serialized
        );
    }

    #[test]
    fn test_yaml_inline_array_object_preservation() {
        let original = r#"services:
  web:
    env_file:
      - path: ./basic.env
        required: true
"#;
        let mut value = parse(original, Format::Yaml).unwrap();
        if let Some(env) = value.pointer_mut("/services/web/env_file/0/required") {
            *env = Value::Bool(false);
        }
        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();
        println!("serialized:\n{}", serialized);

        assert!(
            serialized.lines().any(|l| l == "      - path: ./basic.env"),
            "Expected inline array item, but got:\n{}",
            serialized
        );
        assert!(
            !serialized.lines().any(|l| l == "      -"),
            "Hyphen should not be on its own line! Serialized output:\n{}",
            serialized
        );
    }

    #[test]
    fn test_yaml_key_reordering() {
        let original = "a: 1\nb: 2\nc:\n  d: 3\n  e: 4\n";
        let mut value = parse(original, Format::Yaml).unwrap();

        if let Some(obj) = value.as_object_mut() {
            let mut items: Vec<(String, Value)> = std::mem::take(obj).into_iter().collect();
            items.swap(0, 1); // b, a
            *obj = items.into_iter().collect();
        }

        if let Some(c_obj) = value.pointer_mut("/c").and_then(|v| v.as_object_mut()) {
            let mut items: Vec<(String, Value)> = std::mem::take(c_obj).into_iter().collect();
            items.swap(0, 1); // e, d
            *c_obj = items.into_iter().collect();
        }

        let serialized = serialize(&value, Format::Yaml, Some(original), true).unwrap();
        println!("serialized YAML:\n{}", serialized);

        let idx_b = serialized.find("b:").unwrap();
        let idx_a = serialized.find("a:").unwrap();
        assert!(idx_b < idx_a, "b should come before a");

        let idx_e = serialized.find("e:").unwrap();
        let idx_d = serialized.find("d:").unwrap();
        assert!(idx_e < idx_d, "e should come before d");
    }

    #[test]
    fn test_toml_key_reordering() {
        let original = "a = 1\nb = 2\n[c]\nd = 3\ne = 4\n";
        let mut value = parse(original, Format::Toml).unwrap();

        if let Some(obj) = value.as_object_mut() {
            let mut items: Vec<(String, Value)> = std::mem::take(obj).into_iter().collect();
            items.swap(0, 1); // b, a
            *obj = items.into_iter().collect();
        }

        if let Some(c_obj) = value.pointer_mut("/c").and_then(|v| v.as_object_mut()) {
            let mut items: Vec<(String, Value)> = std::mem::take(c_obj).into_iter().collect();
            items.swap(0, 1); // e, d
            *c_obj = items.into_iter().collect();
        }

        let serialized = serialize(&value, Format::Toml, Some(original), true).unwrap();
        println!("serialized TOML:\n{}", serialized);

        let idx_b = serialized.find("b =").unwrap();
        let idx_a = serialized.find("a =").unwrap();
        assert!(idx_b < idx_a, "b should come before a");

        let idx_e = serialized.find("e =").unwrap();
        let idx_d = serialized.find("d =").unwrap();
        assert!(idx_e < idx_d, "e should come before d");
    }

    #[test]
    fn test_jsonc_parsing() {
        let original = r#"{
            // This is a comment
            "a": 1,
            /* This is a block comment
               spanning multiple lines */
            "b": [
                2,
                3, // trailing comma here
            ],
        }"#;
        let value = parse(original, Format::Jsonc).unwrap();
        assert_eq!(value["a"], Value::from(1));
        assert_eq!(value["b"], serde_json::json!([2, 3]));
    }

    #[test]
    fn test_jsonc_comment_preservation() {
        let original = r#"{
  // This is a header comment
  "a": 1, // Inline comment for a
  /* Block comment for b */
  "b": [
    2, // Comment for index 0
    3
  ]
}"#;
        let mut value = parse(original, Format::Jsonc).unwrap();
        if let Some(obj) = value.as_object_mut() {
            obj.insert("a".to_string(), Value::from(10));
        }
        if let Some(arr) = value.pointer_mut("/b").and_then(|v| v.as_array_mut()) {
            arr.push(Value::from(4));
        }

        let serialized = serialize(&value, Format::Jsonc, Some(original), false).unwrap();

        assert!(serialized.contains("// This is a header comment"));
        assert!(
            serialized.contains("\"a\": 10, // Inline comment for a")
                || serialized.contains("\"a\": 10,// Inline comment for a"),
            "Serialized was: {}",
            serialized
        );
        assert!(serialized.contains("/* Block comment for b */"));
        assert!(
            serialized.contains("2, // Comment for index 0")
                || serialized.contains("2,// Comment for index 0"),
            "Serialized was: {}",
            serialized
        );
        assert!(serialized.contains("4"));
    }

    #[test]
    fn test_jsonc_detect() {
        assert_eq!(detect("test.jsonc", ""), Format::Jsonc);
        assert_eq!(
            detect("test.conf", "{\n  // comment\n  \"a\": 1\n}"),
            Format::Jsonc
        );
        assert_eq!(
            detect("test.conf", "{\n  /* comment */\n  \"a\": 1\n}"),
            Format::Jsonc
        );
        assert_eq!(detect("test.conf", "{\n  \"a\": 1,\n}"), Format::Jsonc);
    }

    #[test]
    fn test_jsonc_multiline_block_comment_preservation() {
        let original = r#"{
  /* * 서버 네트워크 및 연결 설정
   * 내부망과 외부망 포트를 분리하여 보안을 강화함
   */
  "server": {
    "port": 8080
  }
}"#;
        let mut value = parse(original, Format::Jsonc).unwrap();
        if let Some(port) = value.pointer_mut("/server/port") {
            *port = Value::from(9090);
        }
        let serialized = serialize(&value, Format::Jsonc, Some(original), false).unwrap();
        println!("serialized jsonc:\n{}", serialized);

        assert!(serialized.contains("/* * 서버 네트워크 및 연결 설정"));
        assert!(serialized.contains("   * 내부망과 외부망 포트를 분리하여 보안을 강화함"));
        assert!(serialized.contains("   */"));
    }

    #[test]
    fn test_serialize_renamed_keys_preserves_comments() {
        // Test YAML
        let original_yaml = r#"services:
  # This is a comment for web service
  web:
    # Inline comment
    image: nginx # nginx image
"#;
        let mut value = parse(original_yaml, Format::Yaml).unwrap();
        // Rename "web" to "web_service"
        if let Some(services) = value
            .pointer_mut("/services")
            .and_then(|v| v.as_object_mut())
        {
            let web_val = services.remove("web");
            if let Some(web_val) = web_val {
                services.insert("web_service".to_string(), web_val);
            }
        }

        let mut renamed_keys = std::collections::HashMap::new();
        renamed_keys.insert("/services/web_service".to_string(), "web".to_string());

        let serialized_yaml = serialize_with_renames(
            &value,
            Format::Yaml,
            Some(original_yaml),
            false,
            &renamed_keys,
        )
        .unwrap();
        println!("serialized yaml:\n{}", serialized_yaml);
        assert!(
            serialized_yaml.contains("# This is a comment for web service"),
            "Comment lost in YAML!"
        );
        assert!(
            serialized_yaml.contains("web_service:"),
            "Key not renamed in YAML!"
        );

        // Test JSONC
        let original_jsonc = r#"{
  // This is a comment for web service
  "web": {
    "image": "nginx" // nginx image
  }
}"#;
        let mut value_jsonc = parse(original_jsonc, Format::Jsonc).unwrap();
        // Rename "web" to "web_service"
        if let Some(obj) = value_jsonc.as_object_mut() {
            let web_val = obj.remove("web");
            if let Some(web_val) = web_val {
                obj.insert("web_service".to_string(), web_val);
            }
        }

        let mut renamed_keys_jsonc = std::collections::HashMap::new();
        renamed_keys_jsonc.insert("/web_service".to_string(), "web".to_string());

        let serialized_jsonc = serialize_with_renames(
            &value_jsonc,
            Format::Jsonc,
            Some(original_jsonc),
            false,
            &renamed_keys_jsonc,
        )
        .unwrap();
        println!("serialized jsonc:\n{}", serialized_jsonc);
        assert!(
            serialized_jsonc.contains("// This is a comment for web service"),
            "Comment lost in JSONC!"
        );
        assert!(
            serialized_jsonc.contains("\"web_service\":"),
            "Key not renamed in JSONC!"
        );

        // Test TOML
        let original_toml = r#"
# Comment for web
[web]
image = "nginx" # nginx image
"#;
        let mut value_toml = parse(original_toml, Format::Toml).unwrap();
        // Rename "web" to "web_service"
        if let Some(obj) = value_toml.as_object_mut() {
            let web_val = obj.remove("web");
            if let Some(web_val) = web_val {
                obj.insert("web_service".to_string(), web_val);
            }
        }

        let mut renamed_keys_toml = std::collections::HashMap::new();
        renamed_keys_toml.insert("/web_service".to_string(), "web".to_string());

        let serialized_toml = serialize_with_renames(
            &value_toml,
            Format::Toml,
            Some(original_toml),
            false,
            &renamed_keys_toml,
        )
        .unwrap();
        println!("serialized toml:\n{}", serialized_toml);
        assert!(
            serialized_toml.contains("# Comment for web"),
            "Comment lost in TOML!"
        );
        assert!(
            serialized_toml.contains("[web_service]"),
            "Key not renamed in TOML!"
        );
    }

    #[test]
    fn test_yaml_block_scalar_preservation() {
        let original = r#"name: build-test
services:
  app-detailed:
    build:
      dockerfile_inline: |
        FROM docker.io/library/alpine:latest
        RUN echo "inline build"
        CMD ["sleep", "3600"]
      other_field: 123
"#;
        let value = parse(original, Format::Yaml).unwrap();
        let serialized = serialize(&value, Format::Yaml, Some(original), false).unwrap();
        println!("serialized YAML block scalar:\n{}", serialized);

        let expected = r#"name: build-test
services:
  app-detailed:
    build:
      dockerfile_inline: |
        FROM docker.io/library/alpine:latest
        RUN echo "inline build"
        CMD ["sleep", "3600"]
      other_field: 123"#;

        assert_eq!(serialized.trim(), expected.trim());
    }

    #[test]
    fn test_serialize_leaf_yaml_special_starts() {
        assert_eq!(
            serialize_leaf_yaml(&Value::String("#alpine".to_string())),
            "\"#alpine\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("//path".to_string())),
            "\"//path\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("normal".to_string())),
            "normal"
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("with:colon".to_string())),
            "\"with:colon\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("true".to_string())),
            "\"true\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("false".to_string())),
            "\"false\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("True".to_string())),
            "\"True\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("FALSE".to_string())),
            "\"FALSE\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("null".to_string())),
            "\"null\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("Null".to_string())),
            "\"Null\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("~".to_string())),
            "\"~\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("123".to_string())),
            "\"123\""
        );
        assert_eq!(
            serialize_leaf_yaml(&Value::String("45.6".to_string())),
            "\"45.6\""
        );
        assert_eq!(serialize_leaf_yaml(&Value::Null), "");
    }

    #[test]
    fn test_serialize_from_different_format() {
        let original_json = r#"{
  "name": "clise",
  "dependencies": {
    "ratatui": "0.30"
  },
  "features": ["json", "yaml"]
}"#;
        let value = parse(original_json, Format::Json).unwrap();
        let serialized_yaml = serialize(&value, Format::Yaml, None, false).unwrap();
        println!("serialized yaml:\n{}", serialized_yaml);
        assert!(serialized_yaml.contains("name: clise"));
        assert!(serialized_yaml.contains("dependencies:"));
        assert!(serialized_yaml.contains("ratatui: \"0.30\""));
        assert!(serialized_yaml.contains("features:\n  - json\n  - yaml"));
    }
}
