use crate::state::EditorState;

pub fn move_up(state: &mut EditorState) {
    if state.selected > 0 {
        state.selected -= 1;
    }
}

pub fn move_down(state: &mut EditorState) {
    if state.selected + 1 < state.flattened_nodes.len() {
        state.selected += 1;
    }
}

pub fn page_up(state: &mut EditorState) {
    state.selected = state.selected.saturating_sub(state.viewport_height);
}

pub fn page_down(state: &mut EditorState) {
    state.selected =
        (state.selected + state.viewport_height).min(state.flattened_nodes.len().saturating_sub(1));
}

pub fn toggle_expand(state: &mut EditorState) {
    if let Some(node) = state.flattened_nodes.get_mut(state.selected) {
        node.expanded = !node.expanded;
    }
    state.rebuild_flattened();
}

pub fn expand_or_move_to_last_child(state: &mut EditorState) {
    let (is_expanded, path) = match state.flattened_nodes.get(state.selected) {
        Some(n) => (n.expanded, n.path.clone()),
        None => return,
    };

    if !is_expanded {
        // Expand if collapsed
        if let Some(node_mut) = state.flattened_nodes.get_mut(state.selected) {
            node_mut.expanded = true;
        }
        state.rebuild_flattened();
    } else {
        // Move to last direct child if already expanded
        let mut last_child_idx = None;
        let parent_path_len = path.len();

        // Search forward from the current node
        for (i, node) in state
            .flattened_nodes
            .iter()
            .enumerate()
            .skip(state.selected + 1)
        {
            if node.path.starts_with(&path) && node.path.len() == parent_path_len + 1 {
                last_child_idx = Some(i);
            } else if !node.path.starts_with(&path) {
                // Out of current node's subtree
                break;
            }
        }

        if let Some(idx) = last_child_idx {
            state.selected = idx;
        }
    }
}

pub fn collapse_current(state: &mut EditorState) {
    let node = match state.flattened_nodes.get(state.selected) {
        Some(n) => n,
        None => return,
    };

    if node.expanded {
        // Just collapse
        if let Some(node_mut) = state.flattened_nodes.get_mut(state.selected) {
            node_mut.expanded = false;
        }
        state.rebuild_flattened();
    } else if node.depth > 0 {
        // Move to parent
        let current_path = node.path.clone();
        let parent_path = current_path[..current_path.len() - 1].to_vec();

        if let Some(parent_idx) = state
            .flattened_nodes
            .iter()
            .position(|n| n.path == parent_path)
        {
            state.selected = parent_idx;
        }
    }
}

pub fn ensure_visible(state: &mut EditorState, viewport_height: usize) {
    if state.selected < state.scroll_offset {
        state.scroll_offset = state.selected;
    } else if state.selected >= state.scroll_offset + viewport_height {
        state.scroll_offset = state.selected - viewport_height + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use serde_json::json;

    #[test]
    fn test_navigation_up_down() {
        let data = json!({"a": 1, "b": 2});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Initially at root
        assert_eq!(state.selected, 0);

        move_down(&mut state);
        assert_eq!(state.selected, 1);

        move_down(&mut state);
        assert_eq!(state.selected, 2);

        move_down(&mut state); // Boundary
        assert_eq!(state.selected, 2);

        move_up(&mut state);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_collapse_to_parent() {
        let data = json!({"nested": {"key": "value"}});
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Root expanded, nested collapsed
        assert_eq!(state.flattened_nodes.len(), 2);

        state.selected = 1;
        toggle_expand(&mut state); // Expand "nested"
        assert_eq!(state.flattened_nodes.len(), 3);

        state.selected = 2;
        collapse_current(&mut state); // Should move to "nested"
        assert_eq!(state.selected, 1);

        collapse_current(&mut state); // Should collapse "nested"
        assert!(!state.flattened_nodes[1].expanded);
        assert_eq!(state.flattened_nodes.len(), 2);
    }

    #[test]
    fn test_page_navigation() {
        let mut state = EditorState::new(
            json!({
                "1": 1, "2": 2, "3": 3, "4": 4, "5": 5,
                "6": 6, "7": 7, "8": 8, "9": 9, "10": 10
            }),
            Format::Json,
            None,
            None,
        );

        // Initial 0, flattened_nodes should have 11 (root + 10 keys)
        state.viewport_height = 3;

        // PageDown from 0
        page_down(&mut state);
        assert_eq!(state.selected, 3);

        // PageDown again
        page_down(&mut state);
        assert_eq!(state.selected, 6);

        // PageUp
        page_up(&mut state);
        assert_eq!(state.selected, 3);

        // PageDown to end
        state.selected = 9;
        page_down(&mut state);
        assert_eq!(state.selected, 10); // last index

        // PageUp to start
        state.selected = 2;
        page_up(&mut state);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_expand_or_move_to_last_child() {
        let data = json!({
            "parent": {
                "child1": 1,
                "child2": 2
            }
        });
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Root(0) is expanded, "parent"(1) is collapsed initially
        assert_eq!(state.selected, 0);
        state.selected = 1; // "parent"
        assert!(!state.flattened_nodes[1].expanded);

        // 1st press: Expand
        expand_or_move_to_last_child(&mut state);
        assert!(state.flattened_nodes[1].expanded);
        assert_eq!(state.flattened_nodes.len(), 4); // Root, parent, child1, child2
        assert_eq!(state.selected, 1);

        // 2nd press: Move to last direct child ("child2")
        expand_or_move_to_last_child(&mut state);
        assert_eq!(state.selected, 3); // "child2" index

        // 3rd press on leaf should do nothing
        expand_or_move_to_last_child(&mut state);
        assert_eq!(state.selected, 3);
    }

    #[test]
    fn test_preserve_expansion_state_on_toggle() {
        let data = json!({
            "parent": {
                "child": {
                    "leaf": 1
                }
            }
        });
        let mut state = EditorState::new(data, Format::Json, None, None);

        // Root(0) is expanded, "parent"(1) is collapsed
        assert_eq!(state.flattened_nodes.len(), 2);

        state.selected = 1;
        toggle_expand(&mut state); // Expand "parent"
        assert_eq!(state.flattened_nodes.len(), 3); // parent, child (collapsed)

        state.selected = 2; // "child"
        toggle_expand(&mut state); // Expand "child"
        assert_eq!(state.flattened_nodes.len(), 4); // parent, child, leaf

        state.selected = 1; // "parent"
        toggle_expand(&mut state); // Collapse "parent"
        assert_eq!(state.flattened_nodes.len(), 2);

        toggle_expand(&mut state); // Expand "parent" again
        // The expanded state of "child" should be preserved, so "leaf" is also visible
        assert_eq!(state.flattened_nodes.len(), 4);
        assert!(state.flattened_nodes[2].expanded);
    }
}
