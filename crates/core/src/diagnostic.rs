#[cfg(test)]
mod diagnostic_tests {
    use crate::format::Format;
    use crate::state::EditorState;

    #[test]
    #[ignore]
    fn diagnostic_yaml_comments_expanded() {
        let yaml_text = r#"# Server Configuration File
# Version: 1.0

server:
  host: localhost
  port: 8080
  # debug: true
  # timeout: 30
  
database:
  # host: db.example.com
  port: 5432
  name: myapp
  
# TODO: Add caching layer
cache:
  enabled: true
  # ttl: 3600
"#;

        let data = crate::format::parse(yaml_text, Format::Yaml).unwrap();
        let mut state = EditorState::new(
            data.clone(),
            Format::Yaml,
            Some("test.yaml".to_string()),
            Some(yaml_text.to_string()),
        );

        // Expand all nodes
        for node in state.flattened_nodes.iter_mut() {
            if !matches!(node.node_type, crate::state::NodeType::Leaf) {
                node.expanded = true;
            }
        }
        state.rebuild_flattened();

        println!("\n=== YAML Diagnostic (EXPANDED) ===");
        println!("Parsed data: {:#}", data);

        println!("\nFlattened Nodes:");
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let indent = "  ".repeat(node.depth);
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            let expanded_marker = match &node.node_type {
                crate::state::NodeType::Object { .. } | crate::state::NodeType::Array { .. } => {
                    if node.expanded {
                        " ▼"
                    } else {
                        " ▶"
                    }
                }
                _ => "",
            };
            println!(
                "  [{}] {}{}{}: {} (type: {:?}){}",
                i,
                indent,
                node.key,
                expanded_marker,
                node.value_display,
                node.value_type,
                disabled_marker
            );
        }
    }

    #[test]
    #[ignore]
    fn diagnostic_jsonc_comments_expanded() {
        let jsonc_text = r#"{
  // Application settings
  "app": {
    "name": "MyApp",
    "version": "1.0.0",
    // "debug": true,
    // "logLevel": "verbose"
  },
  
  // Server configuration
  "server": {
    "host": "0.0.0.0",
    "port": 3000
    // "ssl": true
  },
  
  // TODO: Add more features
  "features": [
    "auth",
    // "logging",
    "metrics"
  ]
}"#;

        let data = crate::format::parse(jsonc_text, Format::Jsonc).unwrap();
        let mut state = EditorState::new(
            data.clone(),
            Format::Jsonc,
            Some("test.jsonc".to_string()),
            Some(jsonc_text.to_string()),
        );

        // Expand all nodes
        for node in state.flattened_nodes.iter_mut() {
            if !matches!(node.node_type, crate::state::NodeType::Leaf) {
                node.expanded = true;
            }
        }
        state.rebuild_flattened();

        println!("\n=== JSONC Diagnostic (EXPANDED) ===");
        println!("Parsed data: {:#}", data);

        println!("\nFlattened Nodes:");
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let indent = "  ".repeat(node.depth);
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            let expanded_marker = match &node.node_type {
                crate::state::NodeType::Object { .. } | crate::state::NodeType::Array { .. } => {
                    if node.expanded {
                        " ▼"
                    } else {
                        " ▶"
                    }
                }
                _ => "",
            };
            println!(
                "  [{}] {}{}{}: {} (type: {:?}){}",
                i,
                indent,
                node.key,
                expanded_marker,
                node.value_display,
                node.value_type,
                disabled_marker
            );
        }
    }

    #[test]
    #[ignore]
    fn diagnostic_yaml_comments() {
        let yaml_text = r#"# Server Configuration File
# Version: 1.0

server:
  host: localhost
  port: 8080
  # debug: true
  # timeout: 30
  
database:
  # host: db.example.com
  port: 5432
  name: myapp
  
# TODO: Add caching layer
cache:
  enabled: true
  # ttl: 3600
"#;

        let data = crate::format::parse(yaml_text, Format::Yaml).unwrap();
        let state = EditorState::new(
            data.clone(),
            Format::Yaml,
            Some("test.yaml".to_string()),
            Some(yaml_text.to_string()),
        );

        println!("\n=== YAML Diagnostic ===");
        println!("Parsed data: {:#}", data);

        // comment_tree removed in Chunk C+D; diagnostic prints omitted (Phase 5)
        println!("\nFlattened Nodes:");
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let indent = "  ".repeat(node.depth);
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            println!(
                "  [{}] {}{}: {} (type: {:?}, node_type: {:?})",
                i, indent, node.key, node.value_display, node.value_type, node.node_type
            );
            if !disabled_marker.is_empty() {
                print!("{}", disabled_marker);
            }
        }
    }

    #[test]
    #[ignore]
    fn diagnostic_jsonc_comments() {
        let jsonc_text = r#"{
  // Application settings
  "app": {
    "name": "MyApp",
    "version": "1.0.0",
    // "debug": true,
    // "logLevel": "verbose"
  },
  
  // Server configuration
  "server": {
    "host": "0.0.0.0",
    "port": 3000
    // "ssl": true
  },
  
  // TODO: Add more features
  "features": [
    "auth",
    // "logging",
    "metrics"
  ]
}"#;

        let data = crate::format::parse(jsonc_text, Format::Jsonc).unwrap();
        let state = EditorState::new(
            data.clone(),
            Format::Jsonc,
            Some("test.jsonc".to_string()),
            Some(jsonc_text.to_string()),
        );

        println!("\n=== JSONC Diagnostic ===");
        println!("Parsed data: {:#}", data);

        // comment_tree removed in Chunk C+D; diagnostic prints omitted (Phase 5)
        println!("\nFlattened Nodes:");
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let indent = "  ".repeat(node.depth);
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            println!(
                "  [{}] {}{}: {} (type: {:?}, node_type: {:?}){}",
                i,
                indent,
                node.key,
                node.value_display,
                node.value_type,
                node.node_type,
                disabled_marker
            );
        }
    }

    #[test]
    #[ignore]
    fn diagnostic_toggle_comment_yaml() {
        let yaml_text = "a: 1\nb: 2\nc: 3\n";
        let data = crate::format::parse(yaml_text, Format::Yaml).unwrap();
        let mut state = EditorState::new(data, Format::Yaml, None, Some(yaml_text.to_string()));

        println!("\n=== Toggle Comment YAML Diagnostic ===");
        println!("Initial state:");
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            println!(
                "  [{}] {} : {}{}",
                i, node.key, node.value_display, disabled_marker
            );
        }

        state.selected = 2; // Select 'b'
        println!(
            "\nSelected node [{}]: {}",
            state.selected, state.flattened_nodes[state.selected].key
        );

        // Toggle comment (comment out)
        let result = state.toggle_comment();
        println!("\nAfter toggle (comment out): {:?}", result);
        println!("Data: {:#}", state.active_value());
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            println!(
                "  [{}] {} : {}{}",
                i, node.key, node.value_display, disabled_marker
            );
        }

        // Toggle again (uncomment)
        let result = state.toggle_comment();
        println!("\nAfter toggle (uncomment): {:?}", result);
        println!("Data: {:#}", state.active_value());
        for (i, node) in state.flattened_nodes.iter().enumerate() {
            let disabled_marker = if node.is_disabled_comment {
                " [DISABLED]"
            } else {
                ""
            };
            println!(
                "  [{}] {} : {}{}",
                i, node.key, node.value_display, disabled_marker
            );
        }
    }
}
