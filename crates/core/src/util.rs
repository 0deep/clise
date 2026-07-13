/// Convert a path slice to JSON Pointer string (RFC 6901)
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

/// Convert character index to byte index for a string
pub(crate) fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or_else(|| s.len())
}

/// Check if a string value is ambiguous (could be parsed as non-string type)
pub(crate) fn is_ambiguous_string(s: &str) -> bool {
    let s_lower = s.to_lowercase();
    s_lower == "true"
        || s_lower == "false"
        || s_lower == "null"
        || s_lower == "~"
        || s.parse::<i64>().is_ok()
        || s.parse::<u64>().is_ok()
        || s.parse::<f64>().is_ok()
}
