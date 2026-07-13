use crate::format::Format;
use crate::node::find_node_by_path;
use crate::node::{AnnotatedNode, CommentEntryKind as NodeCommentEntryKind};
use crate::state::{EditorState, NodeType, UiNode, ValueType};
use serde_json::Value;

pub fn rebuild_flattened(
    nodes: &[AnnotatedNode],
    root: usize,
    prev_nodes: &[UiNode],
    show_child_counts: bool,
    schema: Option<&Value>,
    format: Format,
) -> Vec<UiNode> {
    let mut out = Vec::new();
    flatten_node(
        nodes,
        root,
        0,
        "root".to_string(),
        prev_nodes,
        &mut out,
        show_child_counts,
        schema,
        format,
    );
    out
}

fn flatten_node(
    nodes: &[AnnotatedNode],
    idx: usize,
    depth: usize,
    key: String,
    prev_nodes: &[UiNode],
    out: &mut Vec<UiNode>,
    show_child_counts: bool,
    schema: Option<&Value>,
    format: Format,
) {
    let Some(node) = nodes.get(idx) else { return };
    let value = &node.value;
    let mut node_type = match value {
        Value::Object(_) => NodeType::Object {
            child_count: node.children.len(),
        },
        Value::Array(_) => NodeType::Array {
            child_count: node.children.len(),
        },
        _ => NodeType::Leaf,
    };
    let mut value_type = match value {
        Value::Null => ValueType::Null,
        Value::Bool(_) => ValueType::Bool,
        Value::Number(_) => ValueType::Number,
        Value::String(_) => ValueType::String,
        Value::Object(_) => ValueType::Object,
        Value::Array(_) => ValueType::Array,
    };
    if matches!(node_type, NodeType::Leaf) {
        infer_type_from_schema(&mut node_type, &mut value_type, value, &node.path, schema);
    }
    let value_display = get_value_display(value, show_child_counts);
    let expanded = match node_type {
        NodeType::Leaf => false,
        _ => prev_nodes
            .iter()
            .find(|n| n.path == node.path)
            .map(|n| n.expanded)
            .unwrap_or(depth == 0),
    };
    // Comments from node.comments
    let has_comment = !node.comments.is_empty();
    let comment_preview = node
        .comments
        .iter()
        .find(|c| {
            matches!(
                c.kind,
                NodeCommentEntryKind::Above | NodeCommentEntryKind::Inline
            )
        })
        .map(|c| {
            let stripped = crate::comment::strip_comment_marker(&c.text, format);
            if stripped.chars().count() > 40 {
                format!("{}...", stripped.chars().take(37).collect::<String>())
            } else {
                stripped
            }
        });
    let is_disabled = !node.is_active;
    let ui = UiNode {
        path: node.path.clone(),
        depth,
        key,
        value_display,
        value_type,
        node_type: node_type.clone(),
        expanded,
        is_disabled_comment: is_disabled,
        has_comment,
        comment_preview,
    };
    out.push(ui);
    if expanded {
        for &child_idx in &node.children {
            let child_key = nodes
                .get(child_idx)
                .and_then(|c| c.path.last().cloned())
                .unwrap_or_default();
            let display_key = if node.value.is_array() {
                format!("[{}]", child_key)
            } else {
                child_key
            };
            flatten_node(
                nodes,
                child_idx,
                depth + 1,
                display_key,
                prev_nodes,
                out,
                show_child_counts,
                schema,
                format,
            );
        }
    }
}

fn infer_type_from_schema(
    node_type: &mut NodeType,
    value_type: &mut ValueType,
    value: &Value,
    path: &[String],
    schema: Option<&Value>,
) {
    if value.is_null() {
        return;
    }
    let Some(s) = schema else {
        return;
    };
    let Some(sub_schema) = crate::schema_util::find_sub_schema(s, path) else {
        return;
    };
    let (_, t) = crate::schema_util::resolve_schema_type_and_default(s, sub_schema);
    let Some(t_str) = t else {
        return;
    };
    if t_str == "array" {
        *node_type = NodeType::Array { child_count: 0 };
        *value_type = ValueType::Array;
    } else if t_str == "object" {
        *node_type = NodeType::Object { child_count: 0 };
        *value_type = ValueType::Object;
    }
}

pub fn count_nodes_per_level(
    nodes: &[AnnotatedNode],
    root: usize,
    depth: usize,
    counts: &mut Vec<usize>,
) {
    let Some(node) = nodes.get(root) else { return };
    if depth >= counts.len() {
        counts.push(0);
    }
    counts[depth] += 1;
    for &child_idx in &node.children {
        count_nodes_per_level(nodes, child_idx, depth + 1, counts);
    }
}

fn get_value_display(value: &Value, show_child_counts: bool) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s),
        Value::Array(arr) => {
            if show_child_counts {
                format!("[{} items]", arr.len())
            } else {
                String::new()
            }
        }
        Value::Object(obj) => {
            if show_child_counts {
                format!("{{{} keys}}", obj.len())
            } else {
                String::new()
            }
        }
    }
}

impl EditorState {
    pub fn rebuild_flattened(&mut self) {
        self.rebuild_flattened_impl(None);
    }

    pub fn rebuild_flattened_impl(&mut self, expand_changed_from: Option<&[AnnotatedNode]>) {
        // 1. cache current expanded
        for node in &self.flattened_nodes {
            if let Some(c) = self
                .all_nodes_cache
                .iter_mut()
                .find(|n| n.path == node.path)
            {
                c.expanded = node.expanded;
            } else {
                self.all_nodes_cache.push(node.clone());
            }
        }
        // 2. auto-expand changed paths
        if let Some(old_nodes) = expand_changed_from {
            let mut changed = Vec::new();
            find_changed_node_paths(old_nodes, &self.nodes, self.root, Vec::new(), &mut changed);
            for path in changed {
                let mut current = Vec::new();
                for part in &path {
                    current.push(part.clone());
                    if let Some(c) = self.all_nodes_cache.iter_mut().find(|n| n.path == current) {
                        c.expanded = true;
                    } else {
                        self.all_nodes_cache.push(UiNode {
                            path: current.clone(),
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
            }
        }
        // 3. rebuild
        self.flattened_nodes = rebuild_flattened(
            &self.nodes,
            self.root,
            &self.all_nodes_cache,
            self.show_child_counts,
            self.schema.as_ref(),
            self.format,
        );
    }
}

/// Diff two node vecs by path (active state or value changes).
fn find_changed_node_paths(
    old_nodes: &[AnnotatedNode],
    new_nodes: &[AnnotatedNode],
    new_idx: usize,
    current_path: Vec<String>,
    changed: &mut Vec<Vec<String>>,
) {
    let Some(new_node) = new_nodes.get(new_idx) else {
        return;
    };
    let old_node = find_node_by_path(old_nodes, &new_node.path).and_then(|i| old_nodes.get(i));
    let differs = match old_node {
        None => true,
        Some(o) => {
            o.is_active != new_node.is_active
                || o.value != new_node.value
                || o.children.len() != new_node.children.len()
        }
    };
    if differs {
        changed.push(current_path.clone());
    }
    for &child_idx in &new_node.children {
        let Some(child) = new_nodes.get(child_idx) else {
            continue;
        };
        let mut cp = current_path.clone();
        cp.push(child.path.last().cloned().unwrap_or_default());
        find_changed_node_paths(old_nodes, new_nodes, child_idx, cp, changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::AnnotatedNode;
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
            comments: Vec::new(),
            children,
            path: path.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_flatten_simple_object() {
        let nodes = vec![
            make_node(
                json!({"name": "test", "active": true}),
                true,
                vec![],
                vec![1, 2],
            ),
            make_node(json!("test"), true, vec!["name"], vec![]),
            make_node(json!(true), true, vec!["active"], vec![]),
        ];
        let out = rebuild_flattened(&nodes, 0, &[], true, None, Format::Yaml);

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].key, "root");
        assert_eq!(out[1].key, "name");
        assert_eq!(out[2].key, "active");
        assert_eq!(out[1].depth, 1);
        assert_eq!(out[2].depth, 1);
    }

    #[test]
    fn test_flatten_nested_object_collapsed_by_default() {
        let nodes = vec![
            make_node(json!({"nested": {"key": "value"}}), true, vec![], vec![1]),
            make_node(json!({"key": "value"}), true, vec!["nested"], vec![2]),
            make_node(json!("value"), true, vec!["nested", "key"], vec![]),
        ];
        let out = rebuild_flattened(&nodes, 0, &[], true, None, Format::Yaml);

        assert_eq!(out.len(), 2);
        assert_eq!(out[1].key, "nested");
        assert!(!out[1].expanded);
    }

    #[test]
    fn test_count_nodes_per_level() {
        //  root (depth 0) -> a (depth 1) -> b, c (depth 2) -> d (depth 3),  e (depth 1)
        let nodes = vec![
            make_node(
                json!({"a": {"b": 1, "c": {"d": 2}}, "e": 3}),
                true,
                vec![],
                vec![1, 4],
            ),
            make_node(json!({"b": 1, "c": {"d": 2}}), true, vec!["a"], vec![2, 3]),
            make_node(json!(1), true, vec!["a", "b"], vec![]),
            make_node(json!({"d": 2}), true, vec!["a", "c"], vec![5]),
            make_node(json!(3), true, vec!["e"], vec![]),
            make_node(json!(2), true, vec!["a", "c", "d"], vec![]),
        ];
        let mut counts = Vec::new();
        count_nodes_per_level(&nodes, 0, 0, &mut counts);
        assert_eq!(counts, vec![1, 2, 2, 1]);
    }

    #[test]
    fn test_flatten_nested_object_expanded() {
        let nodes = vec![
            make_node(json!({"nested": {"key": "value"}}), true, vec![], vec![1]),
            make_node(json!({"key": "value"}), true, vec!["nested"], vec![2]),
            make_node(json!("value"), true, vec!["nested", "key"], vec![]),
        ];
        let prev_nodes = vec![UiNode {
            path: vec!["nested".to_string()],
            depth: 1,
            key: "nested".to_string(),
            value_display: "".to_string(),
            value_type: ValueType::Object,
            node_type: NodeType::Object { child_count: 1 },
            expanded: true,
            is_disabled_comment: false,
            has_comment: false,
            comment_preview: None,
        }];
        let out = rebuild_flattened(&nodes, 0, &prev_nodes, true, None, Format::Yaml);

        assert_eq!(out.len(), 3);
        assert_eq!(out[2].key, "key");
        assert_eq!(out[2].depth, 2);
    }

    #[test]
    fn test_get_value_display_types() {
        assert_eq!(get_value_display(&json!(null), true), "null");
        assert_eq!(get_value_display(&json!(true), true), "true");
        assert_eq!(get_value_display(&json!(123), true), "123");
        assert_eq!(get_value_display(&json!("hello"), true), "\"hello\"");
        assert_eq!(get_value_display(&json!([1, 2, 3]), true), "[3 items]");
        assert_eq!(
            get_value_display(&json!({"a": 1, "b": 2}), true),
            "{2 keys}"
        );
        assert_eq!(get_value_display(&json!([1, 2, 3]), false), "");
        assert_eq!(get_value_display(&json!({"a": 1, "b": 2}), false), "");
    }

    #[test]
    fn test_value_type_detection() {
        let nodes = vec![
            make_node(
                json!({"s": "str", "n": 123, "b": true, "z": null, "o": {}, "a": []}),
                true,
                vec![],
                vec![1, 2, 3, 4, 5, 6],
            ),
            make_node(json!("str"), true, vec!["s"], vec![]),
            make_node(json!(123), true, vec!["n"], vec![]),
            make_node(json!(true), true, vec!["b"], vec![]),
            make_node(json!(null), true, vec!["z"], vec![]),
            make_node(json!({}), true, vec!["o"], vec![]),
            make_node(json!([]), true, vec!["a"], vec![]),
        ];
        let out = rebuild_flattened(&nodes, 0, &[], true, None, Format::Yaml);

        for node in &out {
            match node.key.as_str() {
                "a" => assert_eq!(node.value_type, ValueType::Array),
                "b" => assert_eq!(node.value_type, ValueType::Bool),
                "n" => assert_eq!(node.value_type, ValueType::Number),
                "o" => assert_eq!(node.value_type, ValueType::Object),
                "s" => assert_eq!(node.value_type, ValueType::String),
                "z" => assert_eq!(node.value_type, ValueType::Null),
                "root" => assert_eq!(node.value_type, ValueType::Object),
                _ => {}
            }
        }
    }

    #[test]
    fn test_rebuild_flattened_respects_child_counts_toggle() {
        let nodes = vec![
            make_node(
                json!({"arr": [1, 2], "obj": {"a": 1}}),
                true,
                vec![],
                vec![1, 2],
            ),
            make_node(json!([1, 2]), true, vec!["arr"], vec![3, 4]),
            make_node(json!({"a": 1}), true, vec!["obj"], vec![5]),
            make_node(json!(1), true, vec!["arr", "0"], vec![]),
            make_node(json!(2), true, vec!["arr", "1"], vec![]),
            make_node(json!(1), true, vec!["obj", "a"], vec![]),
        ];

        let out_on = rebuild_flattened(&nodes, 0, &[], true, None, Format::Yaml);
        let arr_node_on = out_on.iter().find(|n| n.key == "arr").unwrap();
        let obj_node_on = out_on.iter().find(|n| n.key == "obj").unwrap();
        assert_eq!(arr_node_on.value_display, "[2 items]");
        assert_eq!(obj_node_on.value_display, "{1 keys}");

        let out_off = rebuild_flattened(&nodes, 0, &[], false, None, Format::Yaml);
        let arr_node_off = out_off.iter().find(|n| n.key == "arr").unwrap();
        let obj_node_off = out_off.iter().find(|n| n.key == "obj").unwrap();
        assert_eq!(arr_node_off.value_display, "");
        assert_eq!(obj_node_off.value_display, "");
    }

    #[test]
    fn test_flatten_disabled_node() {
        let mut nodes = vec![
            make_node(json!({"a": 1, "c": 3}), true, vec![], vec![1, 2, 3]),
            make_node(json!(1), true, vec!["a"], vec![]),
            make_node(json!(null), false, vec!["b"], vec![]), // disabled
            make_node(json!(3), true, vec!["c"], vec![]),
        ];
        nodes[2].comments.push(crate::node::CommentEntry {
            kind: NodeCommentEntryKind::Above,
            text: "# b: 2".to_string(),
            line: 5,
        });

        let out = rebuild_flattened(&nodes, 0, &[], true, None, Format::Yaml);

        // Root + a + # b: 2 + c = 4
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].key, "root");
        assert_eq!(out[1].key, "a");
        assert_eq!(out[2].key, "b");
        assert!(out[2].is_disabled_comment);
        assert_eq!(out[3].key, "c");
    }
}
