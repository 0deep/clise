use crate::state::{EditorState, NodeType, UiNode, ValueType};
use serde_json::Value;

pub fn rebuild_flattened(
    data: &Value,
    prev_nodes: &[UiNode],
    show_child_counts: bool,
    schema: Option<&Value>,
) -> Vec<UiNode> {
    let mut nodes = Vec::new();
    flatten_recursive(
        data,
        Vec::new(),
        0,
        "root".to_string(),
        prev_nodes,
        &mut nodes,
        show_child_counts,
        schema,
    );
    nodes
}

fn flatten_recursive(
    value: &Value,
    path: Vec<String>,
    depth: usize,
    key: String,
    prev_nodes: &[UiNode],
    nodes: &mut Vec<UiNode>,
    show_child_counts: bool,
    schema: Option<&Value>,
) {
    let mut node_type = match value {
        Value::Object(map) => NodeType::Object {
            child_count: map.len(),
        },
        Value::Array(arr) => NodeType::Array {
            child_count: arr.len(),
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

    // InferNodeType using Schema if node is Null
    // Skip this for actual Null values — let the user edit null as a leaf.
    // The oneOf/anyOf variant picker in start_edit_impl handles type selection.
    if let NodeType::Leaf = node_type {
        if !value.is_null() {
            if let Some(s) = schema {
                if let Some(sub_schema) = crate::edit::find_sub_schema(s, &path) {
                    let (_, t) = crate::edit::resolve_schema_type_and_default(s, sub_schema);
                    if let Some(t_str) = t {
                        if t_str == "array" {
                            node_type = NodeType::Array { child_count: 0 };
                            value_type = ValueType::Array;
                        } else if t_str == "object" {
                            node_type = NodeType::Object { child_count: 0 };
                            value_type = ValueType::Object;
                        }
                    }
                }
            }
        }
    }

    let value_display = get_value_display(value, show_child_counts);

    // Find if this path was expanded before
    let expanded = match node_type {
        NodeType::Leaf => false,
        _ => prev_nodes
            .iter()
            .find(|n| n.path == path)
            .map(|n| n.expanded)
            .unwrap_or(depth == 0), // Default to collapsed except root
    };

    let ui_node = UiNode {
        path: path.clone(),
        depth,
        key,
        value_display,
        value_type,
        node_type: node_type.clone(),
        expanded,
    };

    nodes.push(ui_node);

    if expanded {
        match value {
            Value::Object(map) => {
                let keys: Vec<_> = map.keys().collect();
                for k in keys {
                    let mut child_path = path.clone();
                    child_path.push(k.clone());
                    flatten_recursive(
                        &map[k],
                        child_path,
                        depth + 1,
                        k.clone(),
                        prev_nodes,
                        nodes,
                        show_child_counts,
                        schema,
                    );
                }
            }
            Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    let mut child_path = path.clone();
                    child_path.push(i.to_string());
                    flatten_recursive(
                        v,
                        child_path,
                        depth + 1,
                        format!("[{}]", i),
                        prev_nodes,
                        nodes,
                        show_child_counts,
                        schema,
                    );
                }
            }
            _ => {}
        }
    }
}

pub fn count_nodes_per_level(value: &Value, depth: usize, counts: &mut Vec<usize>) {
    if depth >= counts.len() {
        counts.push(0);
    }
    counts[depth] += 1;

    match value {
        Value::Object(map) => {
            for v in map.values() {
                count_nodes_per_level(v, depth + 1, counts);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                count_nodes_per_level(v, depth + 1, counts);
            }
        }
        _ => {}
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

    pub fn rebuild_flattened_impl(&mut self, expand_changed_from: Option<&Value>) {
        // 1. Update current flattened_nodes' expanded states into the cache
        for node in &self.flattened_nodes {
            if let Some(cached_node) = self
                .all_nodes_cache
                .iter_mut()
                .find(|n| n.path == node.path)
            {
                cached_node.expanded = node.expanded;
            } else {
                self.all_nodes_cache.push(node.clone());
            }
        }

        // Auto-expand changed nodes if old_data is provided
        if let Some(old_data) = expand_changed_from {
            let mut changed_paths = Vec::new();
            crate::state::find_changed_paths(old_data, &self.data, Vec::new(), &mut changed_paths);

            for path in changed_paths {
                let mut current = Vec::new();
                for part in &path {
                    current.push(part.clone());
                    if let Some(cached_node) =
                        self.all_nodes_cache.iter_mut().find(|n| n.path == current)
                    {
                        cached_node.expanded = true;
                    } else {
                        self.all_nodes_cache.push(UiNode {
                            path: current.clone(),
                            depth: 0,
                            key: String::new(),
                            value_display: String::new(),
                            value_type: ValueType::Null,
                            node_type: NodeType::Leaf,
                            expanded: true,
                        });
                    }
                }
            }
        }

        // 2. Rebuild using all_nodes_cache as the reference for previous states
        self.flattened_nodes = rebuild_flattened(
            &self.data,
            &self.all_nodes_cache,
            self.show_child_counts,
            self.schema.as_ref(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_flatten_simple_object() {
        let data = json!({
            "name": "test",
            "active": true
        });
        let nodes = rebuild_flattened(&data, &[], true, None);

        // Root + 2 children (preserved insertion order: name, active)
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].key, "root");
        assert_eq!(nodes[1].key, "name");
        assert_eq!(nodes[2].key, "active");
        assert_eq!(nodes[1].depth, 1);
        assert_eq!(nodes[2].depth, 1);
    }

    #[test]
    fn test_flatten_nested_object_collapsed_by_default() {
        let data = json!({
            "nested": {
                "key": "value"
            }
        });
        let nodes = rebuild_flattened(&data, &[], true, None);

        // Root (expanded)
        // nested (collapsed by default)
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].key, "nested");
        assert!(!nodes[1].expanded);
    }

    #[test]
    fn test_count_nodes_per_level() {
        let data = json!({
            "a": {
                "b": 1,
                "c": {
                    "d": 2
                }
            },
            "e": 3
        });
        let mut counts = Vec::new();
        count_nodes_per_level(&data, 0, &mut counts);

        // Depth 0: root (1)
        // Depth 1: a, e (2)
        // Depth 2: b, c (2)
        // Depth 3: d (1)
        assert_eq!(counts, vec![1, 2, 2, 1]);
    }

    #[test]
    fn test_flatten_nested_object_expanded() {
        let data = json!({
            "nested": {
                "key": "value"
            }
        });
        // Pre-expand "nested"
        let prev_nodes = vec![UiNode {
            path: vec!["nested".to_string()],
            depth: 1,
            key: "nested".to_string(),
            value_display: "".to_string(),
            value_type: ValueType::Object,
            node_type: NodeType::Object { child_count: 1 },
            expanded: true,
        }];
        let nodes = rebuild_flattened(&data, &prev_nodes, true, None);

        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[2].key, "key");
        assert_eq!(nodes[2].depth, 2);
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
        let data = json!({
            "s": "str",
            "n": 123,
            "b": true,
            "z": null,
            "o": {},
            "a": []
        });
        let nodes = rebuild_flattened(&data, &[], true, None);

        for node in &nodes {
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
        let data = json!({
            "arr": [1, 2],
            "obj": {"a": 1}
        });

        // With counts (default)
        let nodes_on = rebuild_flattened(&data, &[], true, None);
        let arr_node_on = nodes_on.iter().find(|n| n.key == "arr").unwrap();
        let obj_node_on = nodes_on.iter().find(|n| n.key == "obj").unwrap();
        assert_eq!(arr_node_on.value_display, "[2 items]");
        assert_eq!(obj_node_on.value_display, "{1 keys}");

        // Without counts
        let nodes_off = rebuild_flattened(&data, &[], false, None);
        let arr_node_off = nodes_off.iter().find(|n| n.key == "arr").unwrap();
        let obj_node_off = nodes_off.iter().find(|n| n.key == "obj").unwrap();
        assert_eq!(arr_node_off.value_display, "");
        assert_eq!(obj_node_off.value_display, "");
    }
}
