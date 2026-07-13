use crate::format::Format;
use serde_json::Value;

/// Semantic type of a comment
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentKind {
    /// Description comment for the code below
    Above,
    /// Inline comment on the right side of the same line
    Inline,
    /// Header comment for the entire file (at the very top, before the first node)
    FileHeader,
    /// Trailing comment at the end of the file
    Trailing,
    /// Disabled code (commented-out valid code)
    DisabledCode,
}

/// Individual comment entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentEntry {
    pub kind: CommentKind,
    /// Original comment text (including comment symbols, as is)
    pub text: String,
    /// Original line number (0-indexed)
    pub line: usize,
}

/// Strips comment markers from the text to extract the original code.
pub fn strip_comment_marker(text: &str, format: Format) -> String {
    let trimmed = text.trim_start();
    match format {
        Format::Yaml | Format::Toml => {
            if trimmed.starts_with("# ") {
                trimmed[2..].to_string()
            } else if trimmed.starts_with('#') {
                trimmed[1..].to_string()
            } else {
                trimmed.to_string()
            }
        }
        Format::Jsonc => {
            if trimmed.starts_with("// ") {
                trimmed[3..].to_string()
            } else if trimmed.starts_with("//") {
                trimmed[2..].to_string()
            } else {
                trimmed.to_string()
            }
        }
        _ => trimmed.to_string(),
    }
}

/// Determines if the stripped comment matches a known pattern (e.g., TODO, FIXME).
pub fn is_known_comment_pattern(stripped: &str) -> bool {
    let upper = stripped.trim_start().to_uppercase();
    let patterns = [
        "TODO",
        "FIXME",
        "NOTE",
        "HACK",
        "XXX",
        "WARN",
        "BUG",
        "DESCRIPTION",
        "DESC",
        "INFO",
    ];
    patterns.iter().any(|p| {
        upper.starts_with(p)
            && stripped
                .trim_start()
                .get(p.len()..p.len() + 1)
                .map(|c| c == ":" || c == " " || c == "(" || c.is_empty())
                .unwrap_or(true)
    })
}

/// Check if valid key: value syntax
fn is_valid_kv(stripped: &str, format: Format) -> bool {
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return false;
    }
    match format {
        Format::Yaml => {
            // "key: value" format
            if let Some(colon_pos) = trimmed.find(": ") {
                let key_part = trimmed[..colon_pos].trim();
                return !key_part.is_empty();
            }
            // "key:value" format (no space)
            if let Some(colon_pos) = trimmed.find(':') {
                let key_part = trimmed[..colon_pos].trim();
                let val_part = trimmed[colon_pos + 1..].trim();
                if !key_part.is_empty() && !val_part.is_empty() {
                    let is_valid_key = key_part
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
                    return is_valid_key;
                }
            }
            // "key:" format (ends without value = start of sub-block)
            if trimmed.ends_with(':') && trimmed.len() > 1 {
                let key_part = trimmed[..trimmed.len() - 1].trim();
                return !key_part.is_empty();
            }
            false
        }
        Format::Jsonc => {
            // Must start with "key": format
            trimmed.starts_with('"') && trimmed.contains("\":")
        }
        Format::Toml => {
            if let Some(eq_pos) = trimmed.find('=') {
                let key_part = trimmed[..eq_pos].trim();
                return !key_part.is_empty();
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let name_part = trimmed[1..trimmed.len() - 1].trim();
                return !name_part.is_empty();
            }
            false
        }
        _ => false,
    }
}

/// Check if valid YAML array item syntax
fn is_valid_yaml_array_item(stripped: &str) -> bool {
    let trimmed = stripped.trim();
    trimmed.starts_with("- ") || trimmed == "-"
}

/// Check if valid JSONC array item syntax
fn is_valid_jsonc_array_item(stripped: &str) -> bool {
    serde_json::from_str::<Value>(stripped.trim()).is_ok()
}

/// Determines if a comment is disabled code
///
/// 3-step determination:
/// 1. Strip comment markers
/// 2. Exclude known-patterns (TODO, FIXME, etc.)
/// 3. Verify compatibility with the parent structure
pub fn is_disabled_code(comment_text: &str, format: Format, _parent_value: Option<&Value>) -> bool {
    // Step 1: Strip comment markers
    let stripped = strip_comment_marker(comment_text, format);
    if stripped.trim().is_empty() {
        return false;
    }

    // Step 2: Exclude known-patterns
    if is_known_comment_pattern(&stripped) {
        return false;
    }

    // Step 3: Validate comment syntax
    match format {
        Format::Yaml => is_valid_kv(&stripped, Format::Yaml) || is_valid_yaml_array_item(&stripped),
        Format::Jsonc => {
            is_valid_kv(&stripped, Format::Jsonc) || is_valid_jsonc_array_item(&stripped)
        }
        Format::Toml => is_valid_kv(&stripped, Format::Toml),
        _ => false,
    }
}

/// Classifies a block of consecutive comments before the first node
///
/// Divided by empty lines:
///   Before empty line = FileHeader
///   After empty line = Above (description for the first node)
/// If there are no empty lines:
///   The entire block is treated as Above (conservative approach)
pub fn classify_leading_comments(
    comments: &[(usize, String)],
    format: Format,
    parent_value: Option<&Value>,
) -> Vec<CommentEntry> {
    if comments.is_empty() {
        return Vec::new();
    }

    // Find position of empty line
    let empty_line_idx = comments.iter().position(|(_, text)| text.trim().is_empty());

    let mut result = Vec::new();

    match empty_line_idx {
        Some(idx) => {
            // Before empty line = FileHeader
            for (line, text) in &comments[..idx] {
                result.push(CommentEntry {
                    kind: CommentKind::FileHeader,
                    text: text.clone(),
                    line: *line,
                });
            }
            // Include the empty line itself in FileHeader
            let (line, text) = &comments[idx];
            result.push(CommentEntry {
                kind: CommentKind::FileHeader,
                text: text.clone(),
                line: *line,
            });
            // After empty line = Above or DisabledCode
            for (line, text) in &comments[idx + 1..] {
                let kind =
                    if !text.trim().is_empty() && is_disabled_code(text, format, parent_value) {
                        CommentKind::DisabledCode
                    } else {
                        CommentKind::Above
                    };
                result.push(CommentEntry {
                    kind,
                    text: text.clone(),
                    line: *line,
                });
            }
        }
        None => {
            // No empty line -> all treated as Above or DisabledCode
            for (line, text) in comments {
                let kind =
                    if !text.trim().is_empty() && is_disabled_code(text, format, parent_value) {
                        CommentKind::DisabledCode
                    } else {
                        CommentKind::Above
                    };
                result.push(CommentEntry {
                    kind,
                    text: text.clone(),
                    line: *line,
                });
            }
        }
    }

    result
}

/// Extracts the key name of the disabled code from the comment
pub fn extract_key_from_comment(comment_text: &str, format: Format) -> Option<String> {
    // A disabled comment may span multiple lines (e.g. a commented-out object).
    // Only the FIRST line carries the key; the rest is the nested body.
    let first_line = comment_text.lines().next().unwrap_or("");
    let stripped = strip_comment_marker(first_line, format);
    let trimmed = stripped.trim();
    match format {
        Format::Yaml => {
            if let Some(colon_pos) = trimmed.find(": ") {
                let key = trimmed[..colon_pos].trim();
                if !key.is_empty() {
                    return Some(key.trim_matches('"').trim_matches('\'').to_string());
                }
            }
            if let Some(colon_pos) = trimmed.find(':') {
                let key = trimmed[..colon_pos].trim();
                let val_part = trimmed[colon_pos + 1..].trim();
                if !key.is_empty() && !val_part.is_empty() {
                    let is_valid_key = key
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
                    if is_valid_key {
                        return Some(key.trim_matches('"').trim_matches('\'').to_string());
                    }
                }
            }
            if trimmed.ends_with(':') && trimmed.len() > 1 {
                let key = trimmed[..trimmed.len() - 1].trim();
                if !key.is_empty() {
                    return Some(key.trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }
        Format::Jsonc => {
            if trimmed.starts_with('"') && trimmed.contains("\":") {
                if let Some(colon_pos) = trimmed.find(':') {
                    let key_part = trimmed[..colon_pos].trim();
                    let key = key_part.trim_matches('"').trim();
                    return Some(key.to_string());
                }
            }
        }
        Format::Toml => {
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos]
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- strip_comment_marker ----

    #[test]
    fn test_strip_yaml_comment() {
        assert_eq!(
            strip_comment_marker("# port: 8080", Format::Yaml),
            "port: 8080"
        );
        assert_eq!(
            strip_comment_marker("#port: 8080", Format::Yaml),
            "port: 8080"
        );
        assert_eq!(
            strip_comment_marker("  # port: 8080", Format::Yaml),
            "port: 8080"
        );
    }

    #[test]
    fn test_extract_multiline_disabled_key() {
        let text = "# hermes:\n#   build:\n#     dockerfile: Dockerfile.hermes\n#   image: x";
        let k = extract_key_from_comment(text, Format::Yaml);
        assert_eq!(k, Some("hermes".to_string()));
    }

    #[test]
    fn test_strip_jsonc_comment() {
        assert_eq!(
            strip_comment_marker("// \"port\": 8080", Format::Jsonc),
            "\"port\": 8080"
        );
        assert_eq!(
            strip_comment_marker("//\"port\": 8080", Format::Jsonc),
            "\"port\": 8080"
        );
    }

    // ---- is_known_comment_pattern ----

    #[test]
    fn test_known_patterns() {
        assert!(is_known_comment_pattern("TODO: fix this"));
        assert!(is_known_comment_pattern("FIXME: broken"));
        assert!(is_known_comment_pattern("NOTE: important"));
        assert!(is_known_comment_pattern("HACK something"));
        assert!(is_known_comment_pattern("XXX"));
        assert!(is_known_comment_pattern("TODO(user): fix"));
        assert!(!is_known_comment_pattern("port: 8080"));
        assert!(!is_known_comment_pattern("TODOLIST: items"));
    }

    // ---- is_disabled_code ----

    #[test]
    fn test_disabled_code_yaml_object_context() {
        let parent = serde_json::json!({"existing": "value"});
        // Valid key: value -> code comment
        assert!(is_disabled_code(
            "# port: 8080",
            Format::Yaml,
            Some(&parent)
        ));
        assert!(is_disabled_code(
            "# host: localhost",
            Format::Yaml,
            Some(&parent)
        ));
        // Only key exists (start of sub-block)
        assert!(is_disabled_code("# server:", Format::Yaml, Some(&parent)));
        // General comment -> not code
        assert!(!is_disabled_code(
            "# This is a port setting",
            Format::Yaml,
            Some(&parent)
        ));
        assert!(!is_disabled_code(
            "# Server host description",
            Format::Yaml,
            Some(&parent)
        ));
        // Known pattern -> not code
        assert!(!is_disabled_code(
            "# TODO: needs modification",
            Format::Yaml,
            Some(&parent)
        ));
        assert!(!is_disabled_code(
            "# FIXME: bug",
            Format::Yaml,
            Some(&parent)
        ));
        // Empty comment -> not code
        assert!(!is_disabled_code("#", Format::Yaml, Some(&parent)));
        assert!(!is_disabled_code("# ", Format::Yaml, Some(&parent)));
    }

    #[test]
    fn test_disabled_code_yaml_array_context() {
        let parent = serde_json::json!(["item1", "item2"]);
        // Valid array item -> code comment
        assert!(is_disabled_code(
            "# - disabled_item",
            Format::Yaml,
            Some(&parent)
        ));
        // General comment -> not code
        assert!(!is_disabled_code(
            "# Array description",
            Format::Yaml,
            Some(&parent)
        ));
    }

    #[test]
    fn test_disabled_code_jsonc_object_context() {
        let parent = serde_json::json!({"existing": "value"});
        // Valid "key": value -> code comment
        assert!(is_disabled_code(
            "// \"port\": 8080",
            Format::Jsonc,
            Some(&parent)
        ));
        // General comment -> not code
        assert!(!is_disabled_code(
            "// Server settings",
            Format::Jsonc,
            Some(&parent)
        ));
    }

    #[test]
    fn test_disabled_code_jsonc_array_context() {
        let parent = serde_json::json!([1, 2, 3]);
        // Valid JSON value -> code comment
        assert!(is_disabled_code("// 42", Format::Jsonc, Some(&parent)));
        assert!(is_disabled_code(
            "// \"disabled\"",
            Format::Jsonc,
            Some(&parent)
        ));
        // General comment -> not code
        assert!(!is_disabled_code(
            "// This is an array description",
            Format::Jsonc,
            Some(&parent)
        ));
    }

    #[test]
    fn test_disabled_code_no_parent() {
        // No parent (top-level) -> verify key: value format
        assert!(is_disabled_code("# port: 8080", Format::Yaml, None));
        assert!(!is_disabled_code(
            "# Description comment",
            Format::Yaml,
            None
        ));
    }

    // ---- classify_leading_comments ----

    #[test]
    fn test_classify_with_empty_line_separator() {
        let comments = vec![
            (0, "# ===== Server Settings =====".to_string()),
            (1, "# This file configures the server".to_string()),
            (2, "".to_string()),
            (3, "# Port settings below".to_string()),
        ];
        let result = classify_leading_comments(&comments, Format::Yaml, None);

        assert_eq!(result.len(), 4);
        assert_eq!(result[0].kind, CommentKind::FileHeader);
        assert_eq!(result[1].kind, CommentKind::FileHeader);
        assert_eq!(result[2].kind, CommentKind::FileHeader); // Empty line
        assert_eq!(result[3].kind, CommentKind::Above);
    }

    #[test]
    fn test_classify_without_empty_line() {
        let comments = vec![
            (0, "# Port settings".to_string()),
            (1, "# Default is 8080".to_string()),
        ];
        let result = classify_leading_comments(&comments, Format::Yaml, None);

        assert_eq!(result.len(), 2);
        // If no empty line, all treated as Above
        assert_eq!(result[0].kind, CommentKind::Above);
        assert_eq!(result[1].kind, CommentKind::Above);
    }

    #[test]
    fn test_classify_with_disabled_code_after_separator() {
        let parent = serde_json::json!({"port": 8080});
        let comments = vec![
            (0, "# File header".to_string()),
            (1, "".to_string()),
            (2, "# host: localhost".to_string()),
        ];
        let result = classify_leading_comments(&comments, Format::Yaml, Some(&parent));

        assert_eq!(result[0].kind, CommentKind::FileHeader);
        assert_eq!(result[1].kind, CommentKind::FileHeader); // Empty line
        assert_eq!(result[2].kind, CommentKind::DisabledCode);
    }

    #[test]
    fn test_toml_comments() {
        assert_eq!(
            strip_comment_marker("# key = value", Format::Toml),
            "key = value"
        );
        assert_eq!(
            extract_key_from_comment("# key = value", Format::Toml),
            Some("key".to_string())
        );
        assert!(is_disabled_code("# key = value", Format::Toml, None));
    }
}
