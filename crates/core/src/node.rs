//! AnnotatedNode — Unified node for active/inactive/attached comments.

use serde_json::Value;

/// Attached comment type (not the node's own active/inactive state — axis separation)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentEntryKind {
    /// Description comment above code
    Above,
    /// Inline comment on the right side of the same line
    Inline,
    /// File header at the beginning (attached to root node)
    FileHeader,
    /// Trailing at end of file (attached to root node)
    Trailing,
}

/// One attached comment entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentEntry {
    pub kind: CommentEntryKind,
    /// Original text (including comment symbol as-is)
    pub text: String,
    /// Original line number (0-indexed)
    pub line: usize,
}

/// Unified node — represents active code/inactive code/attached comments
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedNode {
    /// Node value. Inactive nodes also hold the original value before being commented out.
    pub value: Value,
    /// true = active / false = commented out (disabled). Toggle = flip.
    pub is_active: bool,
    /// Attached comments. Does not include inactive node's own text ("# - apple").
    pub comments: Vec<CommentEntry>,
    /// Child node indices (append-only integer indices).
    pub children: Vec<usize>,
    /// JSON Pointer path segments. For external compatibility + parent identification.
    pub path: Vec<String>,
}

impl AnnotatedNode {
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// For jsonschema/validation — inactive items return None
    pub fn active_value(&self) -> Option<&Value> {
        if self.is_active {
            Some(&self.value)
        } else {
            None
        }
    }
}

/// Path-based node search (single O(n) traversal, assuming nodes <10k).
///
/// When several nodes share a path (e.g. a deleted/tombstoned node coexists
/// with a re-added active one after delete+re-add), prefer the ACTIVE node.
/// This keeps value edits and other path-based operations targeting the node
/// the user actually sees, instead of a stale tombstone.
pub fn find_node_by_path(nodes: &[AnnotatedNode], path: &[String]) -> Option<usize> {
    let mut first_inactive: Option<usize> = None;
    for (i, n) in nodes.iter().enumerate() {
        if n.path == path {
            if n.is_active {
                return Some(i);
            }
            if first_inactive.is_none() {
                first_inactive = Some(i);
            }
        }
    }
    first_inactive
}

/// Search for child with specific segment under parent
pub fn find_child_by_segment(
    nodes: &[AnnotatedNode],
    parent_idx: usize,
    segment: &str,
) -> Option<usize> {
    let parent = nodes.get(parent_idx)?;
    for &child_idx in &parent.children {
        let child = nodes.get(child_idx)?;
        if child.path.last().map(|s| s == segment).unwrap_or(false) {
            return Some(child_idx);
        }
    }
    None
}

/// Recursively collect only active children to reconstruct Value tree (for jsonschema validation, etc.)
pub fn nodes_to_active_value(nodes: &[AnnotatedNode], root: usize) -> Value {
    fn build(nodes: &[AnnotatedNode], idx: usize) -> Value {
        let node = match nodes.get(idx) {
            Some(n) => n,
            None => return Value::Null,
        };
        match &node.value {
            Value::Object(_) => {
                let mut map = serde_json::Map::new();
                for &child_idx in &node.children {
                    let child = match nodes.get(child_idx) {
                        Some(c) => c,
                        None => continue,
                    };
                    if !child.is_active {
                        continue;
                    }
                    let key = child.path.last().cloned().unwrap_or_default();
                    map.insert(key, build(nodes, child_idx));
                }
                Value::Object(map)
            }
            Value::Array(_) => {
                let mut arr = Vec::new();
                for &child_idx in &node.children {
                    let child = match nodes.get(child_idx) {
                        Some(c) => c,
                        None => continue,
                    };
                    if !child.is_active {
                        continue;
                    }
                    arr.push(build(nodes, child_idx));
                }
                Value::Array(arr)
            }
            // Leaf nodes: ignore children, use value as-is (only called when active)
            v => v.clone(),
        }
    }
    // Return empty Null when root is inactive? Policy: root is always assumed active. Return value instead of strong-guard.
    if !nodes.get(root).map(|n| n.is_active).unwrap_or(false) {
        return Value::Null;
    }
    build(nodes, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_node(
        value: Value,
        is_active: bool,
        path: Vec<&str>,
        children: Vec<usize>,
    ) -> AnnotatedNode {
        AnnotatedNode {
            value,
            is_active,
            comments: vec![],
            children,
            path: path.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_nodes_to_active_value_skips_disabled() {
        // root: array with 3 children, 1 disabled
        // nodes[0] = root (array), children: [1, 2, 3]
        // nodes[1] = "apple" (active)
        // nodes[2] = "banana" (disabled)
        // nodes[3] = "cherry" (active)
        let nodes = vec![
            make_node(json!([]), true, vec!["items"], vec![1, 2, 3]),
            make_node(json!("apple"), true, vec!["items", "0"], vec![]),
            make_node(json!("banana"), false, vec!["items", "1"], vec![]),
            make_node(json!("cherry"), true, vec!["items", "2"], vec![]),
        ];

        let result = nodes_to_active_value(&nodes, 0);
        let expected = json!(["apple", "cherry"]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_find_node_by_path() {
        let nodes = vec![
            make_node(json!({"a": 1}), true, vec!["a"], vec![]),
            make_node(json!({"b": 2}), true, vec!["a", "b"], vec![]),
            make_node(json!(null), true, vec!["a", "c"], vec![]),
        ];

        // found
        assert_eq!(find_node_by_path(&nodes, &["a".into()]), Some(0));
        assert_eq!(
            find_node_by_path(&nodes, &["a".into(), "b".into()]),
            Some(1)
        );
        assert_eq!(
            find_node_by_path(&nodes, &["a".into(), "c".into()]),
            Some(2)
        );

        // not found
        assert_eq!(find_node_by_path(&nodes, &["x".into()]), None);
        assert_eq!(find_node_by_path(&nodes, &["a".into(), "d".into()]), None);
    }

    #[test]
    fn test_find_child_by_segment() {
        let nodes = vec![
            make_node(json!({}), true, vec!["root"], vec![1, 2]),
            make_node(json!("val1"), true, vec!["root", "alpha"], vec![]),
            make_node(json!("val2"), true, vec!["root", "beta"], vec![]),
        ];

        assert_eq!(find_child_by_segment(&nodes, 0, "alpha"), Some(1));
        assert_eq!(find_child_by_segment(&nodes, 0, "beta"), Some(2));
        assert_eq!(find_child_by_segment(&nodes, 0, "gamma"), None);
        // non-existent parent
        assert_eq!(find_child_by_segment(&nodes, 99, "alpha"), None);
    }
}
