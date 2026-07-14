use crate::comment::{extract_key_from_comment, is_disabled_code, strip_comment_marker};
use crate::node::{
    AnnotatedNode, CommentEntry as NodeCommentEntry, CommentEntryKind as NodeCommentEntryKind,
    nodes_to_active_value,
};
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

/// Parse text into `Vec<AnnotatedNode>`.
///
/// Hybrid: active tree from `parse` + `build_active_nodes`; comments/disabled
/// from line scan so disabled items land as correct siblings with correct values.
///
/// Returns `(nodes, root_idx)`. `root_idx` is usually 0.
pub fn parse_annotated(
    text: &str,
    format: Format,
) -> Result<(Vec<AnnotatedNode>, usize), FormatError> {
    parse_annotated_from_lines(text, format)
}

/// Line-scan annotated parse (Phase 3). Active nodes from serde parse; comments
/// and disabled nodes from source lines; children ordered by source line.
pub fn parse_annotated_from_lines(
    text: &str,
    format: Format,
) -> Result<(Vec<AnnotatedNode>, usize), FormatError> {
    let value = parse(text, format)?;
    let mut nodes: Vec<AnnotatedNode> = Vec::new();
    let root_idx = build_active_nodes(&value, &mut nodes, Vec::new());

    match format {
        Format::Yaml | Format::Toml => {
            scan_yaml_like_comments(text, format, &mut nodes, root_idx);
        }
        Format::Jsonc => {
            scan_jsonc_comments(text, &mut nodes, root_idx);
        }
        Format::Json => {}
    }

    Ok((nodes, root_idx))
}

/// Parse a disabled comment body into a Value (array items → scalar, not array).
fn parse_disabled_value(stripped: &str, format: Format) -> Value {
    match format {
        Format::Yaml | Format::Toml => {
            let t = stripped.trim_start();
            if t.starts_with("- ") {
                return parse_scalar_value(t[2..].trim());
            }
            if t == "-" {
                return Value::Null;
            }
            if let Some(colon_pos) = t.find(": ") {
                let val_part = t[colon_pos + 2..].trim();
                if val_part.is_empty() {
                    return Value::Object(serde_json::Map::new());
                }
                return parse_scalar_value(val_part);
            }
            if t.ends_with(':') && t.len() > 1 {
                return Value::Object(serde_json::Map::new());
            }
            if let Some(colon_pos) = t.find(':') {
                let key_part = t[..colon_pos].trim();
                let val_part = t[colon_pos + 1..].trim();
                if !key_part.is_empty() {
                    if val_part.is_empty() {
                        return Value::Object(serde_json::Map::new());
                    }
                    return parse_scalar_value(val_part);
                }
            }
            parse(t, Format::Yaml).unwrap_or(Value::Null)
        }
        Format::Jsonc => {
            let t = stripped.trim().trim_end_matches(',').trim();
            if t.is_empty() {
                return Value::Null;
            }
            // Direct value (array item): 2, "x", true
            if let Ok(v) = serde_json::from_str::<Value>(t) {
                return v;
            }
            // "key": value → extract value
            if t.starts_with('"') {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&format!("{{{}}}", t))
                {
                    if map.len() == 1 {
                        return map
                            .into_iter()
                            .next()
                            .map(|(_, v)| v)
                            .unwrap_or(Value::Null);
                    }
                }
            }
            Value::Null
        }
        Format::Json => Value::Null,
    }
}

fn parse_scalar_value(s: &str) -> Value {
    let s = s.trim().trim_matches('"').trim_matches('\'');
    if s.is_empty() || s == "null" || s == "~" {
        return Value::Null;
    }
    if s == "true" {
        return Value::Bool(true);
    }
    if s == "false" {
        return Value::Bool(false);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::from(n);
    }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return Value::Number(num);
        }
    }
    Value::String(s.to_string())
}

fn leading_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_yaml_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with('#')
}

fn is_jsonc_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with("//") || trimmed.starts_with("/*")
}

/// Drain pending above comments into node CommentEntry list.
fn drain_pending_as_above(pending: &mut Vec<(usize, String)>) -> Vec<NodeCommentEntry> {
    pending
        .drain(..)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(line, text)| NodeCommentEntry {
            kind: NodeCommentEntryKind::Above,
            text,
            line,
        })
        .collect()
}

/// Attach pending above comments and inline comments to the node at active_idx.
fn attach_comments_to_node(
    nodes: &mut [AnnotatedNode],
    active_idx: usize,
    pending: &mut Vec<(usize, String)>,
    source_lines: &mut std::collections::HashMap<usize, usize>,
    line_idx: usize,
    inline_content: &str,
) {
    let mut above = drain_pending_as_above(pending);
    nodes[active_idx].comments.append(&mut above);
    source_lines.insert(active_idx, line_idx);

    let inline = find_comment_part(inline_content);
    if !inline.is_empty() {
        nodes[active_idx].comments.push(NodeCommentEntry {
            kind: NodeCommentEntryKind::Inline,
            text: inline.to_string(),
            line: line_idx,
        });
    }
}

/// Replace path prefix for a node and all descendants.
fn repath_subtree(
    nodes: &mut [AnnotatedNode],
    idx: usize,
    old_prefix: &[String],
    new_prefix: &[String],
) {
    let path = &nodes[idx].path;
    if path.starts_with(old_prefix) {
        let mut new_path = new_prefix.to_vec();
        new_path.extend_from_slice(&path[old_prefix.len()..]);
        nodes[idx].path = new_path;
    }
    let children = nodes[idx].children.clone();
    for child in children {
        repath_subtree(nodes, child, old_prefix, new_prefix);
    }
}

/// After inserting disabled siblings, renumber array children paths 0..n-1.
pub(crate) fn renumber_array_children(nodes: &mut [AnnotatedNode], parent_idx: usize) {
    if !nodes[parent_idx].value.is_array() {
        return;
    }
    let children = nodes[parent_idx].children.clone();
    let parent_path = nodes[parent_idx].path.clone();
    for (i, &child_idx) in children.iter().enumerate() {
        let old_path = nodes[child_idx].path.clone();
        let mut new_path = parent_path.clone();
        new_path.push(i.to_string());
        if old_path != new_path {
            repath_subtree(nodes, child_idx, &old_path, &new_path);
        }
    }
}

/// Sort each parent's children by source line; renumber array paths.
fn finalize_children_order(
    nodes: &mut [AnnotatedNode],
    source_lines: &std::collections::HashMap<usize, usize>,
) {
    // Collect parent indices that have children
    let parents: Vec<usize> = (0..nodes.len())
        .filter(|&i| !nodes[i].children.is_empty())
        .collect();
    for parent_idx in parents {
        let mut kids: Vec<(usize, usize)> = nodes[parent_idx]
            .children
            .iter()
            .map(|&ci| {
                let line = source_lines.get(&ci).copied().unwrap_or(usize::MAX);
                (ci, line)
            })
            .collect();
        // Stable: preserve relative order when lines equal/missing
        kids.sort_by_key(|&(_, line)| line);
        nodes[parent_idx].children = kids.into_iter().map(|(ci, _)| ci).collect();
        renumber_array_children(nodes, parent_idx);
    }
}

/// YAML/TOML line scan: attach comments and insert disabled siblings.
fn scan_yaml_like_comments(
    text: &str,
    format: Format,
    nodes: &mut Vec<AnnotatedNode>,
    root_idx: usize,
) {
    use std::collections::HashMap;

    let lines: Vec<&str> = text.lines().collect();
    // path_stack: (indent, segment) — last segment of current container path
    let mut path_stack: Vec<(usize, String)> = Vec::new();
    // active-only array counters by parent path pointer
    let mut active_array_idx: HashMap<String, usize> = HashMap::new();
    let mut pending_above: Vec<(usize, String)> = Vec::new();
    let mut leading: Vec<(usize, String)> = Vec::new();
    let mut first_code_seen = false;
    let mut source_lines: HashMap<usize, usize> = HashMap::new();
    // Track open disabled multiline block (choice A: single node)
    let mut in_disabled_block = false;

    source_lines.insert(root_idx, 0);

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = leading_indent(line);

        // Empty line
        if trimmed.is_empty() {
            if !first_code_seen {
                leading.push((i, line.to_string()));
            } else {
                // Attachment boundary for above comments
                pending_above.clear();
            }
            i += 1;
            continue;
        }

        // Comment line
        if is_yaml_comment_line(trimmed) {
            if !first_code_seen {
                leading.push((i, line.to_string()));
                i += 1;
                continue;
            }

            let stripped_probe = strip_comment_marker(line, format);
            // Continuation of disabled multiline block → absorb (choice A)
            if in_disabled_block && is_disabled_continuation(&stripped_probe) {
                i += 1;
                continue;
            }

            // Resolve parent path by indent
            let mut temp_stack = path_stack.clone();
            while let Some(&(stack_indent, _)) = temp_stack.last() {
                if stack_indent >= indent {
                    temp_stack.pop();
                } else {
                    break;
                }
            }
            let parent_path: Vec<String> = temp_stack.iter().map(|(_, k)| k.clone()).collect();
            let parent_idx = if parent_path.is_empty() {
                root_idx
            } else {
                crate::node::find_node_by_path(nodes, &parent_path).unwrap_or(root_idx)
            };
            let parent_value = nodes.get(parent_idx).map(|n| &n.value);

            if is_disabled_code(line, format, parent_value) {
                let stripped = strip_comment_marker(line, format);
                let t = stripped.trim_start();
                let is_array_item = t.starts_with("- ") || t == "-";

                let mut node_path = parent_path.clone();
                let segment = if is_array_item {
                    // Temporary segment; renumber later
                    format!("__d{}", i)
                } else if let Some(key) = extract_key_from_comment(line, format) {
                    key
                } else {
                    format!("disabled_{}", i)
                };
                node_path.push(segment);

                let value = parse_disabled_value(&stripped, format);
                let comments = drain_pending_as_above(&mut pending_above);

                let disabled_node = AnnotatedNode {
                    value,
                    is_active: false,
                    comments,
                    children: Vec::new(),
                    path: node_path,
                };
                let new_idx = nodes.len();
                nodes.push(disabled_node);
                nodes[parent_idx].children.push(new_idx);
                source_lines.insert(new_idx, i);

                // Open multiline block if object start (`key:`)
                let opens_block = (t.ends_with(':') && !t.contains(": "))
                    || (t.ends_with(':')
                        && t.rsplit_once(':')
                            .map(|(_, v)| v.trim().is_empty())
                            .unwrap_or(false));
                in_disabled_block = opens_block;
            } else {
                pending_above.push((i, line.to_string()));
                in_disabled_block = false;
            }
            i += 1;
            continue;
        }

        // --- Active code line ---
        in_disabled_block = false;

        if format == Format::Toml {
            if !first_code_seen {
                first_code_seen = true;
                classify_and_apply_leading(nodes, root_idx, &leading, format);
                leading.clear();
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let is_array_of_tables = trimmed.starts_with("[[") && trimmed.ends_with("]]");
                let content = if is_array_of_tables {
                    &trimmed[2..trimmed.len() - 2]
                } else {
                    &trimmed[1..trimmed.len() - 1]
                };
                let sec_path: Vec<String> =
                    content.split('.').map(|s| s.trim().to_string()).collect();

                path_stack.clear();
                let mut current_path = Vec::new();
                for seg in sec_path {
                    current_path.push(seg.clone());
                    path_stack.push((0, seg));
                }

                if let Some(active_idx) = crate::node::find_node_by_path(nodes, &current_path) {
                    attach_comments_to_node(
                        nodes,
                        active_idx,
                        &mut pending_above,
                        &mut source_lines,
                        i,
                        trimmed,
                    );
                }
                i += 1;
                continue;
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                let mut child_path: Vec<String> =
                    path_stack.iter().map(|(_, k)| k.clone()).collect();
                child_path.push(key.clone());

                if let Some(active_idx) = crate::node::find_node_by_path(nodes, &child_path) {
                    let val_part = trimmed[eq_pos + 1..].trim();
                    attach_comments_to_node(
                        nodes,
                        active_idx,
                        &mut pending_above,
                        &mut source_lines,
                        i,
                        val_part,
                    );
                } else {
                    pending_above.clear();
                }
                i += 1;
                continue;
            } else {
                pending_above.clear();
                i += 1;
                continue;
            }
        }

        // Pop path stack by indent
        while let Some(&(stack_indent, _)) = path_stack.last() {
            if stack_indent >= indent {
                path_stack.pop();
            } else {
                break;
            }
        }

        let parent_path: Vec<String> = path_stack.iter().map(|(_, k)| k.clone()).collect();
        let parent_ptr = crate::util::to_json_pointer(&parent_path);
        let is_array_item = trimmed.starts_with("- ") || trimmed == "-";

        // First code: classify leading → FileHeader / pending Above / disabled
        if !first_code_seen {
            first_code_seen = true;
            classify_and_apply_leading(nodes, root_idx, &leading, format);
            leading.clear();
        }

        let (child_path, enter_container, container_indent) = if is_array_item {
            let idx_ref = active_array_idx.entry(parent_ptr.clone()).or_insert(0);
            let idx = *idx_ref;
            *idx_ref += 1;
            let mut child_path = parent_path.clone();
            child_path.push(idx.to_string());

            // Attach pending above + inline
            if let Some(active_idx) = crate::node::find_node_by_path(nodes, &child_path) {
                let content = if trimmed.starts_with("- ") {
                    &trimmed[2..]
                } else {
                    ""
                };
                attach_comments_to_node(
                    nodes,
                    active_idx,
                    &mut pending_above,
                    &mut source_lines,
                    i,
                    content,
                );
            } else {
                pending_above.clear();
            }

            // Array item may open nested mapping: "- key:" or "- key: val"
            let content = if trimmed.starts_with("- ") {
                trimmed[2..].trim()
            } else {
                ""
            };
            let content_no_comment = {
                let cp = find_comment_part(content);
                if cp.is_empty() {
                    content
                } else {
                    content[..content.len() - cp.len()].trim()
                }
            };
            let mut enter = false;
            if let Some(active_idx) = crate::node::find_node_by_path(nodes, &child_path) {
                if matches!(&nodes[active_idx].value, Value::Object(_) | Value::Array(_)) {
                    // flow containers on one line don't push
                    let is_flow =
                        content_no_comment.starts_with('[') || content_no_comment.starts_with('{');
                    if !is_flow {
                        enter = true;
                    }
                }
            }
            // Nested key on same line: "- name: x" pushes array index then maybe key
            if let Some(c_pos) = content_no_comment.find(": ").or_else(|| {
                if content_no_comment.ends_with(':') {
                    Some(content_no_comment.len() - 1)
                } else {
                    None
                }
            }) {
                let key = content_no_comment[..c_pos]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                let mut key_path = child_path.clone();
                key_path.push(key.clone());
                if let Some(kidx) = crate::node::find_node_by_path(nodes, &key_path) {
                    source_lines.insert(kidx, i);
                    let val_part = content_no_comment[c_pos + 1..].trim();
                    let inline = find_comment_part(val_part);
                    if !inline.is_empty() {
                        nodes[kidx].comments.push(NodeCommentEntry {
                            kind: NodeCommentEntryKind::Inline,
                            text: inline.to_string(),
                            line: i,
                        });
                    }
                }
                // Push array index on stack; if nested container key, push key too
                path_stack.push((indent, child_path.last().cloned().unwrap_or_default()));
                if let Some(kidx) = crate::node::find_node_by_path(nodes, &key_path) {
                    if matches!(&nodes[kidx].value, Value::Object(_) | Value::Array(_)) {
                        let val_part = content_no_comment[c_pos + 1..].trim();
                        let is_flow = val_part.starts_with('[') || val_part.starts_with('{');
                        if !is_flow && (val_part.is_empty() || val_part.starts_with('#')) {
                            path_stack.push((indent + 2, key));
                        }
                    }
                }
                // We already manipulated path_stack
                i += 1;
                continue;
            }

            (child_path, enter, indent)
        } else {
            // key: value
            let colon_res = trimmed.find(": ").or_else(|| {
                if trimmed.ends_with(':') {
                    Some(trimmed.len() - 1)
                } else {
                    None
                }
            });
            if let Some(colon_pos) = colon_res {
                let key = trimmed[..colon_pos]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                let mut child_path = parent_path.clone();
                child_path.push(key.clone());

                if let Some(active_idx) = crate::node::find_node_by_path(nodes, &child_path) {
                    let value_part = trimmed[colon_pos + 1..].trim();
                    attach_comments_to_node(
                        nodes,
                        active_idx,
                        &mut pending_above,
                        &mut source_lines,
                        i,
                        value_part,
                    );

                    let enter =
                        matches!(&nodes[active_idx].value, Value::Object(_) | Value::Array(_));
                    let value_no_c = {
                        let cp = find_comment_part(value_part);
                        if cp.is_empty() {
                            value_part
                        } else {
                            value_part[..value_part.len() - cp.len()].trim()
                        }
                    };
                    let is_flow = value_no_c.starts_with('[') || value_no_c.starts_with('{');
                    let is_block = value_no_c.starts_with('|') || value_no_c.starts_with('>');
                    if is_block {
                        advance_past_block_scalar(&lines, &mut i, indent);
                    }
                    (child_path, enter && !is_flow, indent)
                } else {
                    pending_above.clear();
                    (child_path, false, indent)
                }
            } else {
                // Unrecognized line
                pending_above.clear();
                i += 1;
                continue;
            }
        };

        if enter_container {
            if let Some(seg) = child_path.last() {
                path_stack.push((container_indent, seg.clone()));
            }
        }

        i += 1;
    }

    // Trailing comments after last code
    if !pending_above.is_empty() {
        for (line, text) in pending_above.drain(..) {
            if text.trim().is_empty() {
                continue;
            }
            nodes[root_idx].comments.push(NodeCommentEntry {
                kind: NodeCommentEntryKind::Trailing,
                text,
                line,
            });
        }
    }
    // Leading-only file (no code) — treat as file header on root
    if !first_code_seen && !leading.is_empty() {
        classify_and_apply_leading(nodes, root_idx, &leading, format);
    }

    finalize_children_order(nodes, &source_lines);
}

/// Classify leading comment block onto root (FileHeader) or root-level disabled.
fn classify_and_apply_leading(
    nodes: &mut Vec<AnnotatedNode>,
    root_idx: usize,
    leading: &[(usize, String)],
    format: Format,
) {
    if leading.is_empty() {
        return;
    }
    let empty_idx = leading.iter().position(|(_, t)| t.trim().is_empty());
    let (header_part, rest) = match empty_idx {
        Some(idx) => (&leading[..=idx], &leading[idx + 1..]),
        None => (leading, &[][..]),
    };

    // Non-disabled in header zone → FileHeader; disabled (no blank) → root children
    let mut disabled_buf: Vec<(usize, String)> = Vec::new();
    for (line, text) in header_part {
        if text.trim().is_empty() {
            continue;
        }
        if is_disabled_code(text, format, nodes.get(root_idx).map(|n| &n.value)) {
            if empty_idx.is_none() {
                disabled_buf.push((*line, text.clone()));
            } else {
                // With blank-line split, disabled before blank is unusual — treat as header text
                nodes[root_idx].comments.push(NodeCommentEntry {
                    kind: NodeCommentEntryKind::FileHeader,
                    text: text.clone(),
                    line: *line,
                });
            }
        } else {
            nodes[root_idx].comments.push(NodeCommentEntry {
                kind: NodeCommentEntryKind::FileHeader,
                text: text.clone(),
                line: *line,
            });
        }
    }
    let mut src = std::collections::HashMap::new();
    if !disabled_buf.is_empty() {
        push_disabled_lines(nodes, root_idx, &disabled_buf, format, &mut src);
    }

    // After empty line: disabled at root or Above on root
    let mut rest_disabled: Vec<(usize, String)> = Vec::new();
    for (line, text) in rest {
        if text.trim().is_empty() {
            continue;
        }
        if is_disabled_code(text, format, nodes.get(root_idx).map(|n| &n.value)) {
            rest_disabled.push((*line, text.clone()));
        } else {
            nodes[root_idx].comments.push(NodeCommentEntry {
                kind: NodeCommentEntryKind::Above,
                text: text.clone(),
                line: *line,
            });
        }
    }
    if !rest_disabled.is_empty() {
        push_disabled_lines(nodes, root_idx, &rest_disabled, format, &mut src);
    }
}

/// True if stripped disabled body is a nested continuation (leading spaces remain).
fn is_disabled_continuation(stripped: &str) -> bool {
    !stripped.is_empty() && (stripped.starts_with(' ') || stripped.starts_with('\t'))
}

/// Apply a sequence of leading/pending disabled lines, absorbing indented
/// continuations into the previous disabled object (choice A: single node).
fn push_disabled_lines(
    nodes: &mut Vec<AnnotatedNode>,
    parent_idx: usize,
    lines: &[(usize, String)],
    format: Format,
    source_lines: &mut std::collections::HashMap<usize, usize>,
) {
    let mut in_block = false;
    for (line, text) in lines {
        if text.trim().is_empty() {
            in_block = false;
            continue;
        }
        if !is_disabled_code(text, format, nodes.get(parent_idx).map(|n| &n.value)) {
            in_block = false;
            continue;
        }
        let stripped = strip_comment_marker(text, format);
        // Nested body of previous `# key:` block (e.g. `#   build: x`)
        if in_block && is_disabled_continuation(&stripped) {
            continue;
        }
        let t = stripped.trim_start();
        let is_array_item = t.starts_with("- ") || t == "-";
        let segment = if is_array_item {
            format!("__d{}", line)
        } else {
            extract_key_from_comment(text, format).unwrap_or_else(|| format!("disabled_{}", line))
        };
        let mut path = nodes[parent_idx].path.clone();
        path.push(segment);
        let value = parse_disabled_value(&stripped, format);
        let new_idx = nodes.len();
        nodes.push(AnnotatedNode {
            value,
            is_active: false,
            comments: Vec::new(),
            children: Vec::new(),
            path,
        });
        nodes[parent_idx].children.push(new_idx);
        source_lines.insert(new_idx, *line);

        let opens_block = (t.ends_with(':') && !t.contains(": "))
            || (t.ends_with(':')
                && t.rsplit_once(':')
                    .map(|(_, v)| v.trim().is_empty())
                    .unwrap_or(false));
        in_block = opens_block;
    }
}

/// JSONC line scan for comments and disabled nodes.
fn scan_jsonc_comments(text: &str, nodes: &mut Vec<AnnotatedNode>, root_idx: usize) {
    use std::collections::HashMap;

    let lines: Vec<&str> = text.lines().collect();
    let mut path_stack: Vec<String> = Vec::new();
    let mut active_array_idx: HashMap<String, usize> = HashMap::new();
    let mut pending_above: Vec<(usize, String)> = Vec::new();
    let mut leading: Vec<(usize, String)> = Vec::new();
    let mut first_code_seen = false;
    let mut source_lines: HashMap<usize, usize> = HashMap::new();
    source_lines.insert(root_idx, 0);

    let format = Format::Jsonc;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Block comment start
        if trimmed.starts_with("/*") && !trimmed.contains("*/") {
            if !first_code_seen {
                leading.push((i, line.to_string()));
            } else {
                pending_above.push((i, line.to_string()));
            }
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if !first_code_seen {
                    leading.push((i, next.to_string()));
                } else {
                    pending_above.push((i, next.to_string()));
                }
                if next.contains("*/") {
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }

        if trimmed.is_empty() {
            if !first_code_seen {
                leading.push((i, line.to_string()));
            } else {
                pending_above.clear();
            }
            i += 1;
            continue;
        }

        if is_jsonc_comment_line(trimmed) {
            if !first_code_seen {
                leading.push((i, line.to_string()));
                i += 1;
                continue;
            }

            let parent_path = path_stack.clone();
            let parent_idx = if parent_path.is_empty() {
                root_idx
            } else {
                crate::node::find_node_by_path(nodes, &parent_path).unwrap_or(root_idx)
            };
            let parent_value = nodes.get(parent_idx).map(|n| &n.value);

            if is_disabled_code(line, format, parent_value) {
                let stripped = strip_comment_marker(line, format);
                let t = stripped.trim().trim_end_matches(',').trim();
                let is_kv = t.starts_with('"') && t.contains("\":");
                let segment = if is_kv {
                    extract_key_from_comment(line, format)
                        .unwrap_or_else(|| format!("disabled_{}", i))
                } else {
                    // array item — temporary, renumber later
                    format!("__d{}", i)
                };
                let mut node_path = parent_path.clone();
                node_path.push(segment);
                let value = parse_disabled_value(&stripped, format);
                let comments = drain_pending_as_above(&mut pending_above);
                let new_idx = nodes.len();
                nodes.push(AnnotatedNode {
                    value,
                    is_active: false,
                    comments,
                    children: Vec::new(),
                    path: node_path,
                });
                nodes[parent_idx].children.push(new_idx);
                source_lines.insert(new_idx, i);
            } else {
                pending_above.push((i, line.to_string()));
            }
            i += 1;
            continue;
        }

        // Closing brace/bracket
        if trimmed.starts_with('}') || trimmed.starts_with(']') {
            // Trailing pending after last property → if only comments, may be trailing
            if !path_stack.is_empty() {
                path_stack.pop();
            }
            let comment_part = find_comment_part(trimmed);
            if !comment_part.is_empty() {
                pending_above.push((i, comment_part.to_string()));
            }
            i += 1;
            continue;
        }

        if !first_code_seen {
            first_code_seen = true;
            classify_and_apply_leading(nodes, root_idx, &leading, format);
            leading.clear();
        }

        let comment_part = find_comment_part(trimmed);
        let no_comment = if !comment_part.is_empty() {
            trimmed[..trimmed.len() - comment_part.len()].trim()
        } else {
            trimmed
        };

        // Bare { or [ opening (array/object item container)
        if no_comment == "{"
            || no_comment == "["
            || no_comment == "{},"
            || no_comment == "[],"
            || no_comment.starts_with('{') && no_comment.ends_with(',')
            || no_comment.starts_with('[') && no_comment.ends_with(',')
        {
            // Root document open — do not treat as array element
            if path_stack.is_empty() && (no_comment == "{" || no_comment == "[") {
                source_lines.insert(root_idx, i);
                i += 1;
                continue;
            }
            let parent_ptr = crate::util::to_json_pointer(&path_stack);
            let parent_is_array = {
                let pidx = if path_stack.is_empty() {
                    root_idx
                } else {
                    crate::node::find_node_by_path(nodes, &path_stack).unwrap_or(root_idx)
                };
                nodes.get(pidx).map(|n| n.value.is_array()).unwrap_or(false)
            };
            if parent_is_array {
                let idx_ref = active_array_idx.entry(parent_ptr).or_insert(0);
                let idx = *idx_ref;
                *idx_ref += 1;
                let mut child_path = path_stack.clone();
                child_path.push(idx.to_string());
                if let Some(aidx) = crate::node::find_node_by_path(nodes, &child_path) {
                    let mut above = drain_pending_as_above(&mut pending_above);
                    nodes[aidx].comments.append(&mut above);
                    source_lines.insert(aidx, i);
                    if !comment_part.is_empty() {
                        nodes[aidx].comments.push(NodeCommentEntry {
                            kind: NodeCommentEntryKind::Inline,
                            text: comment_part.to_string(),
                            line: i,
                        });
                    }
                }
                let opens_multi = (no_comment == "{" || no_comment == "[")
                    || ((no_comment.starts_with('{') || no_comment.starts_with('['))
                        && !no_comment.contains('}')
                        && !no_comment.contains(']'));
                if opens_multi {
                    path_stack.push(idx.to_string());
                }
            }
            i += 1;
            continue;
        }

        // "key": value
        let mut key = String::new();
        let mut colon_pos = None;
        let mut in_str = false;
        let mut escape = false;
        let chars: Vec<char> = no_comment.chars().collect();
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
            } else if c == '"' {
                in_str = true;
            } else if c == ':' {
                colon_pos = Some(char_idx);
                break;
            }
            char_idx += 1;
        }

        if let Some(_c_pos) = colon_pos {
            let mut child_path = path_stack.clone();
            child_path.push(key.clone());
            if let Some(aidx) = crate::node::find_node_by_path(nodes, &child_path) {
                let mut above = drain_pending_as_above(&mut pending_above);
                nodes[aidx].comments.append(&mut above);
                source_lines.insert(aidx, i);
                if !comment_part.is_empty() {
                    nodes[aidx].comments.push(NodeCommentEntry {
                        kind: NodeCommentEntryKind::Inline,
                        text: comment_part.to_string(),
                        line: i,
                    });
                }
                let is_container = matches!(&nodes[aidx].value, Value::Object(_) | Value::Array(_));
                if is_container {
                    // enter if value is open brace/bracket on this or next structure
                    let after = no_comment[_c_pos + 1..].trim();
                    let opens = after.starts_with('{') || after.starts_with('[');
                    let closed_same = (after.starts_with('{') && after.contains('}'))
                        || (after.starts_with('[') && after.contains(']'));
                    if (opens && !closed_same) || after.is_empty() || after == "{" || after == "[" {
                        path_stack.push(key);
                    }
                }
            } else {
                pending_above.clear();
            }
        } else {
            // Maybe array scalar item: 1, "x", true
            let parent_ptr = crate::util::to_json_pointer(&path_stack);
            let parent_is_array = {
                let pidx = if path_stack.is_empty() {
                    root_idx
                } else {
                    crate::node::find_node_by_path(nodes, &path_stack).unwrap_or(root_idx)
                };
                nodes.get(pidx).map(|n| n.value.is_array()).unwrap_or(false)
            };
            if parent_is_array {
                let val_str = no_comment.trim().trim_end_matches(',');
                if serde_json::from_str::<Value>(val_str).is_ok()
                    || (!val_str.is_empty()
                        && !val_str.starts_with('{')
                        && !val_str.starts_with('['))
                {
                    let idx_ref = active_array_idx.entry(parent_ptr).or_insert(0);
                    let idx = *idx_ref;
                    *idx_ref += 1;
                    let mut child_path = path_stack.clone();
                    child_path.push(idx.to_string());
                    if let Some(aidx) = crate::node::find_node_by_path(nodes, &child_path) {
                        let mut above = drain_pending_as_above(&mut pending_above);
                        nodes[aidx].comments.append(&mut above);
                        source_lines.insert(aidx, i);
                        if !comment_part.is_empty() {
                            nodes[aidx].comments.push(NodeCommentEntry {
                                kind: NodeCommentEntryKind::Inline,
                                text: comment_part.to_string(),
                                line: i,
                            });
                        }
                    }
                }
            }
        }

        i += 1;
    }

    if !pending_above.is_empty() {
        for (line, text) in pending_above.drain(..) {
            if text.trim().is_empty() {
                continue;
            }
            nodes[root_idx].comments.push(NodeCommentEntry {
                kind: NodeCommentEntryKind::Trailing,
                text,
                line,
            });
        }
    }
    if !first_code_seen && !leading.is_empty() {
        classify_and_apply_leading(nodes, root_idx, &leading, format);
    }

    finalize_children_order(nodes, &source_lines);
}

/// Build AnnotatedNode vec from a bare Value (no comments, all active).
/// Used by EditorState::new when original_text is None.
pub fn value_to_annotated(value: &Value) -> (Vec<AnnotatedNode>, usize) {
    let mut nodes: Vec<AnnotatedNode> = Vec::new();
    let root = build_active_nodes(value, &mut nodes, Vec::new());
    (nodes, root)
}

/// Recursively build active nodes in pre-order DFS.
/// Returns the index of the newly pushed node.
fn build_active_nodes(value: &Value, nodes: &mut Vec<AnnotatedNode>, path: Vec<String>) -> usize {
    let idx = nodes.len();

    match value {
        Value::Object(map) => {
            nodes.push(AnnotatedNode {
                value: value.clone(),
                is_active: true,
                comments: Vec::new(),
                children: Vec::new(),
                path: path.clone(),
            });
            for (key, child_val) in map {
                let mut child_path = path.clone();
                child_path.push(key.clone());
                let child_idx = build_active_nodes(child_val, nodes, child_path);
                nodes[idx].children.push(child_idx);
            }
        }
        Value::Array(arr) => {
            nodes.push(AnnotatedNode {
                value: value.clone(),
                is_active: true,
                comments: Vec::new(),
                children: Vec::new(),
                path: path.clone(),
            });
            for (i, child_val) in arr.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(i.to_string());
                let child_idx = build_active_nodes(child_val, nodes, child_path);
                nodes[idx].children.push(child_idx);
            }
        }
        _ => {
            // Scalar (null, bool, number, string)
            nodes.push(AnnotatedNode {
                value: value.clone(),
                is_active: true,
                comments: Vec::new(),
                children: Vec::new(),
                path,
            });
        }
    }

    idx
}

fn reindent_comment(comment_text: &str, indent_size: usize) -> String {
    let trimmed = comment_text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!("{}{}", " ".repeat(indent_size), trimmed)
}

pub fn serialize_annotated(
    nodes: &[AnnotatedNode],
    root: usize,
    format: Format,
) -> Result<String, FormatError> {
    match format {
        Format::Yaml => {
            let mut serializer = YamlSerializer {
                nodes,
                output: String::new(),
            };
            serializer.emit_node(root, 0, false, false, true)?;
            Ok(serializer.output)
        }
        Format::Jsonc => {
            let mut serializer = JsoncSerializer {
                nodes,
                output: String::new(),
            };
            serializer.emit_node(root, 0, false, false, true)?;
            Ok(serializer.output)
        }
        Format::Toml => {
            let mut serializer = TomlSerializer {
                nodes,
                output: String::new(),
            };
            serializer.emit_node(root, true)?;
            Ok(serializer.output)
        }
        Format::Json => {
            let value = nodes_to_active_value(nodes, root);
            let mut output = serde_json::to_string_pretty(&value)?;
            output.push('\n');
            Ok(output)
        }
    }
}

fn serialize_toml_value(val: &Value) -> String {
    match val {
        Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "\"\"".to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(serialize_toml_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(_) => "".to_string(),
    }
}

struct TomlSerializer<'a> {
    nodes: &'a [AnnotatedNode],
    output: String,
}

impl<'a> TomlSerializer<'a> {
    fn emit_node(&mut self, idx: usize, parent_active: bool) -> Result<(), FormatError> {
        let node = &self.nodes[idx];
        let is_active = node.is_active && parent_active;
        let comment_prefix = if is_active { "" } else { "# " };

        // root
        if node.path.is_empty() {
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::FileHeader {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }
            for &child in &node.children {
                self.emit_node(child, is_active)?;
            }
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Trailing {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }
            return Ok(());
        }

        // 1. Above comments
        for c in &node.comments {
            if c.kind == NodeCommentEntryKind::Above {
                self.output.push_str(&format!("{}\n", c.text.trim()));
            }
        }

        // 2. Body
        let is_object = matches!(node.value, Value::Object(_));
        let is_array = matches!(node.value, Value::Array(_));
        let is_aot = is_array
            && node.children.iter().any(|&ci| {
                self.nodes
                    .get(ci)
                    .map(|c| matches!(c.value, Value::Object(_)))
                    .unwrap_or(false)
            });
        let is_container = is_object;

        if is_container {
            let path_str = node.path.join(".");
            let mut line = format!("{}[{}]", comment_prefix, path_str);
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Inline {
                    line.push_str("  ");
                    line.push_str(c.text.trim());
                }
            }
            self.output.push_str(&format!("{}\n", line));

            for &child in &node.children {
                self.emit_node(child, is_active)?;
            }
        } else if is_aot {
            // Array-of-tables: emit [[key]] for each object child
            let parent_path_prefix: Vec<&str> = node.path.iter().map(|s| s.as_str()).collect();
            for &child_idx in &node.children {
                if let Some(child) = self.nodes.get(child_idx) {
                    if !matches!(child.value, Value::Object(_)) {
                        continue;
                    }
                    // Emit above comments for the child
                    for c in &child.comments {
                        if c.kind == NodeCommentEntryKind::Above {
                            self.output
                                .push_str(&format!("{}\n", reindent_comment(&c.text, 0)));
                        }
                    }
                    let child_active = child.is_active && is_active;
                    let child_comment_prefix = if child_active { "" } else { "# " };
                    // Build path for [[full.path]]
                    let path_str = parent_path_prefix.join(".");
                    let mut line = format!("{}[[{}]]", child_comment_prefix, path_str);
                    for c in &child.comments {
                        if c.kind == NodeCommentEntryKind::Inline {
                            line.push_str("  ");
                            line.push_str(c.text.trim());
                        }
                    }
                    self.output.push_str(&format!("{}\n", line));
                    for &grandchild in &child.children {
                        self.emit_node(grandchild, child_active)?;
                    }
                }
            }
        } else {
            let key = node.path.last().map(|s| s.as_str()).unwrap_or("");
            let val_str = serialize_toml_value(&node.value);
            let mut line = format!("{}{} = {}", comment_prefix, key, val_str);
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Inline {
                    line.push_str("  ");
                    line.push_str(c.text.trim());
                }
            }
            self.output.push_str(&format!("{}\n", line));
        }

        Ok(())
    }
}

struct JsoncSerializer<'a> {
    nodes: &'a [AnnotatedNode],
    output: String,
}

impl<'a> JsoncSerializer<'a> {
    fn emit_node(
        &mut self,
        idx: usize,
        indent: usize,
        is_parent_array: bool,
        is_last_child: bool,
        parent_active: bool,
    ) -> Result<(), FormatError> {
        let node = &self.nodes[idx];
        let is_active = node.is_active && parent_active;
        let leading_indent = " ".repeat(indent);
        let comment_prefix = if is_active { "" } else { "// " };

        // Special handling for root node
        if node.path.is_empty() && indent == 0 {
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::FileHeader {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }

            let open_bracket = if matches!(node.value, Value::Array(_)) {
                "["
            } else {
                "{"
            };
            let close_bracket = if matches!(node.value, Value::Array(_)) {
                "]"
            } else {
                "}"
            };

            self.output
                .push_str(&format!("{}{}\n", comment_prefix, open_bracket));

            let next_is_parent_array = matches!(node.value, Value::Array(_));
            let len = node.children.len();
            for (i, &child) in node.children.iter().enumerate() {
                self.emit_node(child, 2, next_is_parent_array, i == len - 1, is_active)?;
            }

            self.output
                .push_str(&format!("{}{}\n", comment_prefix, close_bracket));

            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Trailing {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }
            return Ok(());
        }

        // 1. Above comments
        for c in &node.comments {
            if c.kind == NodeCommentEntryKind::Above {
                self.output
                    .push_str(&format!("{}\n", reindent_comment(&c.text, indent)));
            }
        }

        // 2. Body & Children
        let is_container = matches!(node.value, Value::Object(_) | Value::Array(_));
        let is_empty_container = is_container && node.children.is_empty();

        if is_container && !is_empty_container {
            let open_bracket = if matches!(node.value, Value::Array(_)) {
                "["
            } else {
                "{"
            };
            let close_bracket = if matches!(node.value, Value::Array(_)) {
                "]"
            } else {
                "}"
            };
            let suffix = if is_last_child { "" } else { "," };

            let body = if is_parent_array {
                format!("{}{}{}", leading_indent, comment_prefix, open_bracket)
            } else {
                let key = node.path.last().map(|s| s.as_str()).unwrap_or("");
                format!(
                    "{}{}\"{}\": {}",
                    leading_indent, comment_prefix, key, open_bracket
                )
            };

            let mut line = body;
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Inline {
                    line.push_str("  ");
                    line.push_str(c.text.trim());
                }
            }
            self.output.push_str(&format!("{}\n", line));

            let next_is_parent_array = matches!(node.value, Value::Array(_));
            let len = node.children.len();
            for (i, &child) in node.children.iter().enumerate() {
                self.emit_node(
                    child,
                    indent + 2,
                    next_is_parent_array,
                    i == len - 1,
                    is_active,
                )?;
            }

            self.output.push_str(&format!(
                "{}{}{}{}\n",
                leading_indent, comment_prefix, close_bracket, suffix
            ));
        } else {
            let val_str = if is_empty_container {
                if matches!(node.value, Value::Object(_)) {
                    "{}".to_string()
                } else {
                    "[]".to_string()
                }
            } else {
                serde_json::to_string(&node.value)?
            };
            let suffix = if is_last_child { "" } else { "," };

            let body = if is_parent_array {
                format!("{}{}{}{}", leading_indent, comment_prefix, val_str, suffix)
            } else {
                let key = node.path.last().map(|s| s.as_str()).unwrap_or("");
                format!(
                    "{}{}\"{}\": {}{}",
                    leading_indent, comment_prefix, key, val_str, suffix
                )
            };

            let mut line = body;
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Inline {
                    line.push_str("  ");
                    line.push_str(c.text.trim());
                }
            }
            self.output.push_str(&format!("{}\n", line));
        }

        Ok(())
    }
}

struct YamlSerializer<'a> {
    nodes: &'a [AnnotatedNode],
    output: String,
}

impl<'a> YamlSerializer<'a> {
    fn emit_node(
        &mut self,
        idx: usize,
        indent: usize,
        is_parent_array: bool,
        is_first_field_of_array_item: bool,
        parent_active: bool,
    ) -> Result<(), FormatError> {
        let node = &self.nodes[idx];
        let is_active = node.is_active && parent_active;
        let leading_indent = " ".repeat(indent);
        let parent_indent = " ".repeat(indent.saturating_sub(2));

        // Special handling for root node
        if node.path.is_empty() && indent == 0 {
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::FileHeader {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }
            let is_root_array = matches!(node.value, Value::Array(_));
            for &child in &node.children {
                self.emit_node(child, 0, is_root_array, false, is_active)?;
            }
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Trailing {
                    self.output.push_str(&format!("{}\n", c.text.trim()));
                }
            }
            return Ok(());
        }

        // 1. Above comments
        let above_indent = if is_parent_array || is_first_field_of_array_item {
            indent.saturating_sub(2)
        } else {
            indent
        };
        for c in &node.comments {
            if c.kind == NodeCommentEntryKind::Above {
                self.output
                    .push_str(&format!("{}\n", reindent_comment(&c.text, above_indent)));
            }
        }

        // 2. Body
        let is_container = matches!(node.value, Value::Object(_) | Value::Array(_));
        let is_empty_container = is_container && node.children.is_empty();

        if is_container && !is_empty_container {
            if is_parent_array {
                // Object/array elements in array
                for (i, &child) in node.children.iter().enumerate() {
                    self.emit_node(
                        child,
                        indent,
                        matches!(node.value, Value::Array(_)),
                        i == 0,
                        is_active,
                    )?;
                }
            } else {
                let key = node.path.last().map(|s| s.as_str()).unwrap_or("");
                let body = if is_first_field_of_array_item {
                    if is_active {
                        format!("{}- {}:", parent_indent, key)
                    } else {
                        format!("{}# - {}:", parent_indent, key)
                    }
                } else {
                    if is_active {
                        format!("{}{}:", leading_indent, key)
                    } else {
                        format!("{}# {}:", leading_indent, key)
                    }
                };

                let mut line = body;
                for c in &node.comments {
                    if c.kind == NodeCommentEntryKind::Inline {
                        line.push_str("  ");
                        line.push_str(c.text.trim());
                    }
                }
                self.output.push_str(&format!("{}\n", line));

                let next_indent = if matches!(node.value, Value::Array(_)) {
                    indent + 4
                } else {
                    indent + 2
                };
                let next_is_parent_array = matches!(node.value, Value::Array(_));
                for &child in &node.children {
                    self.emit_node(child, next_indent, next_is_parent_array, false, is_active)?;
                }
            }
        } else {
            let val_str = if is_empty_container {
                if is_parent_array {
                    if matches!(node.value, Value::Object(_)) {
                        "{}".to_string()
                    } else {
                        "[]".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                serialize_leaf_yaml(&node.value)
            };

            let body = if is_parent_array {
                if is_active {
                    format!("{}- {}", parent_indent, val_str)
                } else {
                    format!("{}# - {}", parent_indent, val_str)
                }
            } else {
                let key = node.path.last().map(|s| s.as_str()).unwrap_or("");
                let val_suffix = if val_str.is_empty() {
                    "".to_string()
                } else {
                    format!(" {}", val_str)
                };
                if is_first_field_of_array_item {
                    if is_active {
                        format!("{}- {}:{}", parent_indent, key, val_suffix)
                    } else {
                        format!("{}# - {}:{}", parent_indent, key, val_suffix)
                    }
                } else {
                    if is_active {
                        format!("{}{}:{}", leading_indent, key, val_suffix)
                    } else {
                        format!("{}# {}:{}", leading_indent, key, val_suffix)
                    }
                }
            };

            let mut line = body;
            for c in &node.comments {
                if c.kind == NodeCommentEntryKind::Inline {
                    line.push_str("  ");
                    line.push_str(c.text.trim());
                }
            }
            self.output.push_str(&format!("{}\n", line));
        }

        Ok(())
    }
}

fn is_valid_scheme(prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
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
fn advance_past_block_scalar(lines: &[&str], i: &mut usize, min_indent: usize) {
    while *i + 1 < lines.len() {
        let next_line = lines[*i + 1];
        let next_trimmed = next_line.trim_start();
        if next_trimmed.is_empty() {
            *i += 1;
        } else {
            let next_indent = next_line.len() - next_trimmed.len();
            if next_indent > min_indent {
                *i += 1;
            } else {
                break;
            }
        }
    }
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
    fn test_serialize_yaml_active_only() {
        let yaml = "name: test\nport: 8080\nitems:\n  - a\n  - b\n";
        let (nodes, root) = parse_annotated(yaml, Format::Yaml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        assert_eq!(serialized, yaml);
    }

    #[test]
    fn test_serialize_yaml_disabled() {
        let yaml = "name: test\n# port: 8080\nitems:\n  - a\n  # - b\n";
        let (nodes, root) = parse_annotated(yaml, Format::Yaml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        assert_eq!(serialized, yaml);
    }

    #[test]
    fn test_serialize_yaml_comments() {
        let yaml = "# header\n# above name\nname: test  # inline name\n# above port\n# port: 8080\nitems:  # inline items\n  # above a\n  - a\n  # - b\n# tail\n";
        let (nodes, root) = parse_annotated(yaml, Format::Yaml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        assert_eq!(serialized, yaml);
    }

    #[test]
    fn test_serialize_jsonc_active_only() {
        let jsonc = "{\n  \"name\": \"test\",\n  \"port\": 8080,\n  \"items\": [\n    \"a\",\n    \"b\"\n  ]\n}\n";
        let (nodes, root) = parse_annotated(jsonc, Format::Jsonc).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Jsonc).unwrap();
        assert_eq!(serialized, jsonc);
    }

    #[test]
    fn test_serialize_jsonc_disabled() {
        let jsonc = "{\n  \"name\": \"test\",\n  // \"port\": 8080,\n  \"items\": [\n    \"a\",\n    // \"b\"\n  ]\n}\n";
        let (nodes, root) = parse_annotated(jsonc, Format::Jsonc).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Jsonc).unwrap();
        assert_eq!(serialized, jsonc);
    }

    #[test]
    fn test_serialize_jsonc_comments() {
        let jsonc = "// header\n// above name\n{\n  \"name\": \"test\",  // inline name\n  // above port\n  // \"port\": 8080,\n  \"items\": [  // inline items\n    // above a\n    \"a\",\n    // \"b\"\n  ]\n}\n// tail\n";
        let (nodes, root) = parse_annotated(jsonc, Format::Jsonc).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Jsonc).unwrap();
        assert_eq!(serialized, jsonc);
    }

    #[test]
    fn test_serialize_toml_basic() {
        let toml = "# header\ntitle = \"My App\"\n# above db\n[database]\n# db host\nhost = \"localhost\"\nport = 5432  # port info\n# tail\n";
        let (nodes, root) = parse_annotated(toml, Format::Toml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Toml).unwrap();
        assert_eq!(serialized, toml);
    }

    #[test]
    fn test_serialize_roundtrip_yaml() {
        let yaml = "# header\n# above name\nname: test  # inline name\n# above port\n# port: 8080\nitems:  # inline items\n  # above a\n  - a\n  # - b\n# tail\n";
        let (nodes, root) = parse_annotated(yaml, Format::Yaml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        let (nodes2, root2) = parse_annotated(&serialized, Format::Yaml).unwrap();
        let serialized2 = serialize_annotated(&nodes2, root2, Format::Yaml).unwrap();
        assert_eq!(serialized, serialized2);
    }

    #[test]
    fn test_serialize_roundtrip_jsonc() {
        let jsonc = "// header\n// above name\n{\n  \"name\": \"test\",  // inline name\n  // above port\n  // \"port\": 8080,\n  \"items\": [  // inline items\n    // above a\n    \"a\",\n    // \"b\"\n  ]\n}\n// tail\n";
        let (nodes, root) = parse_annotated(jsonc, Format::Jsonc).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Jsonc).unwrap();
        let (nodes2, root2) = parse_annotated(&serialized, Format::Jsonc).unwrap();
        let serialized2 = serialize_annotated(&nodes2, root2, Format::Jsonc).unwrap();
        assert_eq!(serialized, serialized2);
    }

    #[test]
    fn test_serialize_yaml_root_array() {
        let yaml = "- 1\n- 2\n- 3\n";
        let (nodes, root) = parse_annotated(yaml, Format::Yaml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        assert_eq!(serialized, yaml);
    }

    #[test]
    fn test_serialize_json_basic() {
        let json = "{\n  \"a\": 1,\n  \"b\": 2\n}\n";
        let (nodes, root) = parse_annotated(json, Format::Json).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Json).unwrap();
        assert_eq!(serialized, json);
    }

    #[test]
    fn test_serialize_roundtrip_toml() {
        let toml = "# header\ntitle = \"My App\"\n# above db\n[database]\n# db host\nhost = \"localhost\"\nport = 5432  # port info\n# tail\n";
        let (nodes, root) = parse_annotated(toml, Format::Toml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Toml).unwrap();
        let (nodes2, root2) = parse_annotated(&serialized, Format::Toml).unwrap();
        let serialized2 = serialize_annotated(&nodes2, root2, Format::Toml).unwrap();
        assert_eq!(serialized, serialized2);
    }

    #[test]
    fn test_serialize_toml_aot() {
        let toml = "[[items]]\na = 1\n[[items]]\na = 2\n";
        let (nodes, root) = parse_annotated(toml, Format::Toml).unwrap();
        let serialized = serialize_annotated(&nodes, root, Format::Toml).unwrap();
        assert!(
            serialized.contains("[[items]]"),
            "Expected AoT headers, got: {}",
            serialized
        );
    }

    #[test]
    fn test_serialize_yaml_empty_containers() {
        let mut nodes = Vec::new();
        let root = build_active_nodes(
            &serde_json::json!({"empty_obj": {}, "empty_arr": []}),
            &mut nodes,
            Vec::new(),
        );
        let serialized = serialize_annotated(&nodes, root, Format::Yaml).unwrap();
        assert_eq!(serialized, "empty_obj:\nempty_arr:\n");
    }
}
