use crate::edit::find_sub_schema;
use crate::state::{EditMode, EditorState, NodeType, ValueType};
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Widget};

/// SchemaEditor Widget
pub struct SchemaEditor<'a> {
    theme: &'a Theme,
    block: Option<Block<'a>>,
}

impl<'a> SchemaEditor<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self { theme, block: None }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl<'a> StatefulWidget for SchemaEditor<'a> {
    type State = EditorState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.tooltip_area = None;
        state.dropdown_area = None;
        let inner_area = match self.block {
            Some(b) => {
                let inner = b.inner(area);
                b.render(area, buf);
                inner
            }
            None => area,
        };

        if inner_area.height < 2 {
            return;
        }

        let list_area = Rect::new(
            inner_area.x + 1,
            inner_area.y,
            inner_area.width.saturating_sub(2),
            inner_area.height - 1,
        );
        let status_area = Rect::new(
            inner_area.x + 1,
            inner_area.y + inner_area.height - 1,
            inner_area.width.saturating_sub(2),
            1,
        );

        state.viewport_height = list_area.height as usize;

        let show_cursor =
            if state.last_cursor_activity.elapsed() < std::time::Duration::from_millis(500) {
                true
            } else {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() % 1000 < 500)
                    .unwrap_or(true)
            };

        let (tip_area, dropdown_area) = render_list(list_area, buf, state, self.theme, show_cursor);
        state.tooltip_area = tip_area;
        state.dropdown_area = dropdown_area;

        // Render scrollbar if content exceeds viewport
        let total_nodes = state.flattened_nodes.len();
        let max_scroll = total_nodes.saturating_sub(state.viewport_height);
        if max_scroll > 0 {
            use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
            let mut scrollbar_state = ScrollbarState::new(max_scroll).position(state.scroll_offset);
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("┐"))
                .end_symbol(Some("┘"))
                .track_symbol(Some("░"))
                .thumb_symbol("█")
                .style(Style::default().fg(Color::DarkGray));
            scrollbar.render(area, buf, &mut scrollbar_state);
        }

        render_status_bar(status_area, buf, state, self.theme, show_cursor);

        if matches!(state.edit_mode, EditMode::SavePrompt { .. }) {
            render_save_prompt(area, buf, state, self.theme);
        }

        if matches!(state.edit_mode, EditMode::Help) {
            render_help_modal(area, buf, state, self.theme);
        }
    }
}

fn render_help_modal(area: Rect, buf: &mut Buffer, _state: &EditorState, theme: &Theme) {
    let shortcuts = [
        ("?", "Show this help"),
        ("/", "Search key/value"),
        ("S", "Save changes"),
        ("U", "Undo"),
        ("R", "Redo"),
        ("T", "Toggle type hints"),
        ("K", "Toggle child counts"),
        ("Up/Down", "Navigate through nodes"),
        ("Ctrl+Up/Down", "Move node up/down"),
        ("PgUp/PgDn", "Scroll page up/down"),
        ("Left", "Collapse node"),
        ("Right", "Expand node / Move to last child"),
        ("Space", "Toggle expansion"),
        ("Enter", "Add child (Obj/Arr) or Edit leaf"),
        ("Backspace", "Edit and clear current value"),
        ("Delete or D", "Delete selected node"),
        ("Esc or Q", "Quit (prompts to save if modified)"),
        ("Ctrl+C", "Force quit without saving"),
    ];

    let width = 60.min(area.width);
    let height = ((shortcuts.len() + 4) as u16).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    ratatui::widgets::Clear.render(popup_area, buf);

    let block = Block::bordered()
        .title(" Keyboard Shortcuts ")
        .border_style(theme.focused_style);
    block.render(popup_area, buf);

    for (i, (key, desc)) in shortcuts.iter().enumerate() {
        let row_y = y + 2 + i as u16;
        if row_y >= y + height - 1 {
            break;
        }

        buf.set_string(x + 2, row_y, key, theme.key_style);
        buf.set_string(x + 15, row_y, desc, theme.string_style);
    }

    let footer = "Press any key to close";
    buf.set_string(
        x + (width - footer.len() as u16) / 2,
        y + height - 1,
        footer,
        Style::default().fg(Color::DarkGray),
    );
}

fn render_save_prompt(area: Rect, buf: &mut Buffer, state: &EditorState, theme: &Theme) {
    let selected = match state.edit_mode {
        EditMode::SavePrompt { selected } => selected,
        _ => 0,
    };

    let width = 40.min(area.width);
    let height = 8.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    ratatui::widgets::Clear.render(popup_area, buf);

    let block = Block::bordered().border_style(theme.focused_style);
    block.render(popup_area, buf);

    let msg = "Save changes before exiting?";
    let no_text = "[N]o";
    let yes_text = "[Y]es";

    buf.set_string(
        x + (width - msg.len() as u16) / 2,
        y + 3,
        msg,
        Style::default().add_modifier(Modifier::BOLD),
    );

    // Spaced out buttons: "[N]o    [Y]es"
    let buttons_width = no_text.len() as u16 + 4 + yes_text.len() as u16;
    let buttons_x = x + (width - buttons_width) / 2;
    let buttons_y = y + 5;
    let no_x = buttons_x;
    let yes_x = buttons_x + no_text.len() as u16 + 4;

    let base_style = Style::default().fg(Color::DarkGray);
    let highlight_key_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let no_style = if selected == 0 {
        theme.focused_style.add_modifier(Modifier::REVERSED)
    } else {
        base_style
    };
    let yes_style = if selected == 1 {
        theme.focused_style.add_modifier(Modifier::REVERSED)
    } else {
        base_style
    };

    buf.set_string(no_x, buttons_y, no_text, no_style);
    buf.set_string(yes_x, buttons_y, yes_text, yes_style);

    // Highlight shortcut keys with White color if not selected
    if selected != 0 {
        buf.set_style(Rect::new(no_x + 1, buttons_y, 1, 1), highlight_key_style);
    }
    if selected != 1 {
        buf.set_style(Rect::new(yes_x + 1, buttons_y, 1, 1), highlight_key_style);
    }
}

fn render_list(
    area: Rect,
    buf: &mut Buffer,
    state: &mut EditorState,
    theme: &Theme,
    show_cursor: bool,
) -> (Option<Rect>, Option<Rect>) {
    if state.flattened_nodes.is_empty() {
        return (None, None);
    }

    // 1. Calculate height of the selected node (for scrolling)
    let mut selected_lines: u16 = 1;
    if let Some(node) = state.selected_node() {
        let x_offset = (node.depth as u16).saturating_mul(2);
        let colon_x = x_offset
            .saturating_add(2)
            .saturating_add(node.key.len() as u16);

        let mut type_hint_len = 0;
        if state.show_type_hints {
            let hint = if let Some(schema) = &state.schema {
                if let Some(sub) = find_sub_schema(schema, &node.path) {
                    let node_pointer = crate::state::to_json_pointer(&node.path);
                    let node_value = state.data.pointer(&node_pointer);
                    extract_type_hint_for_value(sub, node_value)
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            };
            type_hint_len = hint.len() as u16;
            if type_hint_len == 0 {
                type_hint_len = 9; // Approximate "[String]" etc
            }
        }

        let first_line_val_x = area.x + colon_x + 2 + type_hint_len;
        let first_line_width = area.right().saturating_sub(first_line_val_x) as usize;
        let wrapped_val_x = area.x + x_offset + 2;
        let wrapped_line_width = area.right().saturating_sub(wrapped_val_x) as usize;

        let text_to_measure = match &state.edit_mode {
            EditMode::TextPrompt { buffer, .. } | EditMode::NewKeyPrompt { buffer, .. } => {
                Some(buffer.as_str())
            }
            EditMode::Normal => Some(node.value_display.as_str()),
            _ => None,
        };

        if let Some(text) = text_to_measure {
            if text.len() > first_line_width && first_line_width > 0 {
                let remaining = text.len() - first_line_width;
                if wrapped_line_width > 0 {
                    selected_lines =
                        1 + ((remaining + wrapped_line_width - 1) / wrapped_line_width) as u16;
                }
            }
        }

        if let Some(schema) = &state.schema {
            if let Some(sub) = find_sub_schema(schema, &node.path) {
                if let Some(desc) = extract_description(sub) {
                    let node_x = area.x.saturating_add(x_offset);
                    let max_tip_width = area
                        .right()
                        .saturating_sub(node_x)
                        .saturating_sub(2)
                        .clamp(20, 60);
                    let tip_lines = wrap_text(&desc, max_tip_width as usize).len();
                    let display_lines = tip_lines.min(8);
                    selected_lines += (display_lines + 2) as u16;
                }
            }
        }
    }

    // 2. Adjust scroll_offset
    if state.scroll_to_selected {
        if state.selected < state.scroll_offset {
            state.scroll_offset = state.selected;
        } else {
            // Find how many lines are visible from scroll_offset
            let mut current_y: u16 = 0;
            let mut idx = state.scroll_offset;
            let mut found = false;

            while idx < state.flattened_nodes.len() {
                let lines = if idx == state.selected {
                    selected_lines
                } else {
                    1
                };
                if current_y + lines > area.height {
                    break;
                }
                if idx == state.selected {
                    found = true;
                    break;
                }
                current_y += lines;
                idx += 1;
            }

            if !found {
                // Must scroll down
                state.scroll_offset = state.selected;
                let mut total_lines = selected_lines;
                while state.scroll_offset > 0 {
                    let prev_idx = state.scroll_offset - 1;
                    if total_lines + 1 > area.height {
                        break;
                    }
                    total_lines += 1;
                    state.scroll_offset = prev_idx;
                }
            }
        }
        state.scroll_to_selected = false;
    }

    // 3. Render
    let mut current_y: u16 = 0;
    let mut node_offset = state.scroll_offset;
    let mut edit_overlay_info = None;
    let mut selected_render_y = None;
    let mut selected_render_x = None;
    let mut selected_render_height = None;
    let mut selected_render_bg = None;

    while current_y < area.height && node_offset < state.flattened_nodes.len() {
        let node = &state.flattened_nodes[node_offset];
        let is_selected = node_offset == state.selected;
        let lines_used = if is_selected { selected_lines } else { 1 };

        if current_y + lines_used > area.height {
            break;
        }

        let y = area.y + current_y;
        let x_offset = (node.depth as u16).saturating_mul(2);

        let prefix = match node.node_type {
            NodeType::Object { .. } | NodeType::Array { .. } => {
                if node.expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            }
            NodeType::Leaf => "  ",
        };

        let is_hovered = state.hovered_node == Some(node_offset);
        let is_modified = {
            let base_modified = state.is_node_modified(&node.path);
            if is_selected {
                match &state.edit_mode {
                    EditMode::TextPrompt { buffer, .. } => {
                        let pointer = crate::state::to_json_pointer(&node.path);
                        if let Some(orig_val) = state.original_data.pointer(&pointer) {
                            let curr_val = if let Ok(parsed) =
                                serde_json::from_str::<serde_json::Value>(buffer)
                            {
                                parsed
                            } else {
                                serde_json::Value::String(buffer.clone())
                            };
                            curr_val != *orig_val
                        } else {
                            true
                        }
                    }
                    EditMode::RenameKeyPrompt { buffer, .. } => node.key != *buffer,
                    _ => base_modified,
                }
            } else {
                base_modified
            }
        };
        let modify_bg = Color::Rgb(30, 58, 138); // Dark blue background for modified items
        let hover_bg = Color::Rgb(50, 50, 50); // Dark gray background for hovered items

        let item_bg = if is_hovered && !is_selected {
            Some(hover_bg)
        } else if is_modified && !is_selected {
            Some(modify_bg)
        } else {
            None
        };

        if let Some(bg) = item_bg {
            let bg_style = Style::default().bg(bg);
            for ry in y..(y.saturating_add(lines_used)) {
                for rx in area.x..(area.x.saturating_add(x_offset)) {
                    if rx < area.right() {
                        buf[(rx, ry)].set_style(bg_style);
                    }
                }
            }
        }

        let mut prefix_style = if is_selected {
            theme.focused_style
        } else {
            theme.bracket_style
        };
        if let Some(bg) = item_bg {
            prefix_style = prefix_style.bg(bg);
        }
        buf.set_string(area.x.saturating_add(x_offset), y, prefix, prefix_style);

        let wrapped_val_x = area.x.saturating_add(x_offset).saturating_add(2);
        let wrapped_line_width = area.right().saturating_sub(wrapped_val_x) as usize;

        let is_editing_key = match &state.edit_mode {
            EditMode::NewKeyPrompt {
                parent_path,
                temp_key,
                ..
            } => node.path.starts_with(parent_path) && node.path.last() == Some(temp_key),
            EditMode::NewKeyDropdown {
                parent_path,
                temp_key,
                ..
            } => node.path.starts_with(parent_path) && node.path.last() == Some(temp_key),
            EditMode::RenameKeyPrompt {
                parent_path,
                original_key,
                ..
            } => node.path.starts_with(parent_path) && node.path.last() == Some(original_key),
            _ => false,
        };

        let mut key_style = if is_selected {
            theme.focused_style
        } else {
            theme.key_style
        };
        if let Some(bg) = item_bg {
            key_style = key_style.bg(bg);
        }

        let mut value_style = if is_selected {
            theme.focused_style
        } else {
            match node.value_type {
                ValueType::String => theme.string_style,
                ValueType::Number => theme.number_style,
                ValueType::Bool => theme.bool_style,
                ValueType::Null => theme.null_style,
                ValueType::Object | ValueType::Array => theme.bracket_style,
            }
        };
        if let Some(bg) = item_bg {
            value_style = value_style.bg(bg);
        }

        if is_editing_key {
            match &state.edit_mode {
                EditMode::NewKeyPrompt {
                    buffer, cursor_pos, ..
                }
                | EditMode::RenameKeyPrompt {
                    buffer, cursor_pos, ..
                } => {
                    let key_x = area.x.saturating_add(x_offset).saturating_add(2);
                    let max_width = area.right().saturating_sub(key_x) as usize;
                    render_wrapped_text(
                        buf,
                        area,
                        y,
                        key_x,
                        max_width,
                        wrapped_val_x,
                        wrapped_line_width,
                        buffer,
                        key_style,
                        Some(*cursor_pos),
                        show_cursor,
                        state.search_query.as_deref(),
                    );
                }
                EditMode::NewKeyDropdown {
                    filter_buffer,
                    cursor_pos,
                    ..
                } => {
                    let display_text = if filter_buffer.is_empty() {
                        "(Select Key)"
                    } else {
                        filter_buffer
                    };
                    let key_x = area.x.saturating_add(x_offset).saturating_add(2);
                    let max_width = area.right().saturating_sub(key_x) as usize;
                    let text_style = if filter_buffer.is_empty() {
                        Style::default().fg(ratatui::style::Color::DarkGray)
                    } else {
                        key_style
                    };
                    let mut final_style = text_style;
                    if let Some(bg) = item_bg {
                        final_style = final_style.bg(bg);
                    }

                    render_wrapped_text(
                        buf,
                        area,
                        y,
                        key_x,
                        max_width,
                        wrapped_val_x,
                        wrapped_line_width,
                        display_text,
                        final_style,
                        Some(*cursor_pos),
                        show_cursor,
                        state.search_query.as_deref(),
                    );
                }
                _ => {
                    render_highlighted_line(
                        buf,
                        area.x.saturating_add(x_offset).saturating_add(2),
                        y,
                        &node.key,
                        wrapped_line_width,
                        key_style,
                        state.search_query.as_deref(),
                    );
                }
            }
        } else {
            render_highlighted_line(
                buf,
                area.x.saturating_add(x_offset).saturating_add(2),
                y,
                &node.key,
                wrapped_line_width,
                key_style,
                state.search_query.as_deref(),
            );
        }

        let mut type_hint_text = String::new();
        if state.show_type_hints && !is_editing_key {
            if let Some(schema) = &state.schema {
                if let Some(sub) = find_sub_schema(schema, &node.path) {
                    let node_pointer = crate::state::to_json_pointer(&node.path);
                    let node_value = state.data.pointer(&node_pointer);
                    type_hint_text = extract_type_hint_for_value(sub, node_value);
                }
            }
        }

        let actual_key_len = if is_editing_key {
            match &state.edit_mode {
                EditMode::NewKeyPrompt { buffer, .. }
                | EditMode::RenameKeyPrompt { buffer, .. } => {
                    unicode_width::UnicodeWidthStr::width(buffer.as_str()) as u16
                }
                EditMode::NewKeyDropdown { .. } => 12, // "(Select Key)" length
                _ => unicode_width::UnicodeWidthStr::width(node.key.as_str()) as u16,
            }
        } else {
            unicode_width::UnicodeWidthStr::width(node.key.as_str()) as u16
        };

        let mut hint_style = Style::default().fg(ratatui::style::Color::DarkGray);
        if let Some(bg) = item_bg {
            hint_style = hint_style.bg(bg);
        }

        if !type_hint_text.is_empty() {
            buf.set_string(
                area.x
                    .saturating_add(x_offset)
                    .saturating_add(2)
                    .saturating_add(actual_key_len),
                y,
                &type_hint_text,
                hint_style,
            );
        }

        let type_hint_width = unicode_width::UnicodeWidthStr::width(type_hint_text.as_str()) as u16;
        let colon_x = area
            .x
            .saturating_add(x_offset)
            .saturating_add(2)
            .saturating_add(actual_key_len)
            .saturating_add(type_hint_width);

        let mut colon_style = theme.bracket_style;
        if is_selected {
            colon_style = theme.focused_style;
        }
        if let Some(bg) = item_bg {
            colon_style = colon_style.bg(bg);
        }
        if colon_x < area.right() {
            buf.set_string(colon_x, y, ": ", colon_style);
        }

        let first_line_val_x = colon_x.saturating_add(2); // Position after ": " (default)

        if is_selected {
            selected_render_y = Some(y);
            selected_render_x = Some(area.x.saturating_add(x_offset));
            selected_render_bg = item_bg;
        }

        let first_line_width = area.right().saturating_sub(first_line_val_x) as usize;

        if is_selected {
            let mut node_height = 1;
            let text_to_measure = match &state.edit_mode {
                EditMode::TextPrompt { buffer, .. } | EditMode::NewKeyPrompt { buffer, .. } => {
                    Some(buffer.as_str())
                }
                EditMode::Normal => Some(node.value_display.as_str()),
                _ => None,
            };
            if let Some(text) = text_to_measure {
                if text.len() > first_line_width && first_line_width > 0 {
                    let remaining = text.len() - first_line_width;
                    if wrapped_line_width > 0 {
                        node_height =
                            1 + ((remaining + wrapped_line_width - 1) / wrapped_line_width) as u16;
                    }
                }
            }
            selected_render_height = Some(node_height);
        }

        // Render Value (with wrapping if editing or selected)
        if first_line_val_x < area.right() {
            match &state.edit_mode {
                EditMode::TextPrompt { buffer, cursor_pos } if is_selected => {
                    if buffer.is_empty() {
                        let placeholder = state.schema.as_ref().and_then(|s| {
                            find_sub_schema(s, &node.path).and_then(format_type_placeholder)
                        });
                        if let Some(ref ph) = placeholder {
                            let mut ph_style = Style::default().fg(Color::DarkGray);
                            if let Some(bg) = item_bg {
                                ph_style = ph_style.bg(bg);
                            }
                            buf.set_string(first_line_val_x, y, ph, ph_style);
                            if show_cursor && *cursor_pos == 0 {
                                if let Some(cell) = buf.cell_mut((first_line_val_x, y)) {
                                    cell.set_style(ph_style.add_modifier(Modifier::REVERSED));
                                }
                            }
                        } else {
                            render_wrapped_text(
                                buf,
                                area,
                                y,
                                first_line_val_x,
                                first_line_width,
                                wrapped_val_x,
                                wrapped_line_width,
                                buffer,
                                value_style,
                                Some(*cursor_pos),
                                show_cursor,
                                state.search_query.as_deref(),
                            );
                        }
                    } else {
                        render_wrapped_text(
                            buf,
                            area,
                            y,
                            first_line_val_x,
                            first_line_width,
                            wrapped_val_x,
                            wrapped_line_width,
                            buffer,
                            value_style,
                            Some(*cursor_pos),
                            show_cursor,
                            state.search_query.as_deref(),
                        );
                    }
                }
                EditMode::Dropdown {
                    options,
                    descriptions,
                    selected,
                    scroll_offset,
                    filter_buffer,
                    filtered_indices,
                } if is_selected => {
                    let display_text = if filter_buffer.is_empty() {
                        &node.value_display
                    } else {
                        filter_buffer
                    };
                    render_wrapped_text(
                        buf,
                        area,
                        y,
                        first_line_val_x,
                        first_line_width,
                        wrapped_val_x,
                        wrapped_line_width,
                        display_text,
                        value_style,
                        if filter_buffer.is_empty() {
                            None
                        } else {
                            Some(filter_buffer.chars().count())
                        },
                        show_cursor,
                        state.search_query.as_deref(),
                    );
                    let filtered: Vec<String> = filtered_indices
                        .iter()
                        .map(|&i| options[i].clone())
                        .collect();
                    let filtered_descs: Vec<Option<String>> = filtered_indices
                        .iter()
                        .map(|&i| descriptions[i].clone())
                        .collect();
                    edit_overlay_info = Some((
                        first_line_val_x,
                        y,
                        filtered,
                        filtered_descs,
                        selected,
                        *scroll_offset,
                        filter_buffer.clone(),
                    ));
                }
                EditMode::NewKeyPrompt { .. } if is_selected => {
                    buf.set_string(first_line_val_x, y, "null", value_style);
                }
                EditMode::RenameKeyPrompt { .. } if is_selected => {
                    render_highlighted_line(
                        buf,
                        first_line_val_x,
                        y,
                        &node.value_display,
                        first_line_width,
                        value_style,
                        state.search_query.as_deref(),
                    );
                }
                EditMode::NewKeyDropdown {
                    options,
                    descriptions,
                    selected,
                    scroll_offset,
                    filter_buffer,
                    filtered_indices,
                    ..
                } if is_selected => {
                    buf.set_string(first_line_val_x, y, "null", value_style);
                    let filtered: Vec<String> = filtered_indices
                        .iter()
                        .map(|&i| options[i].clone())
                        .collect();
                    let filtered_descs: Vec<Option<String>> = filtered_indices
                        .iter()
                        .map(|&i| descriptions[i].clone())
                        .collect();
                    edit_overlay_info = Some((
                        area.x.saturating_add(x_offset).saturating_add(2),
                        y,
                        filtered,
                        filtered_descs,
                        selected,
                        *scroll_offset,
                        filter_buffer.clone(),
                    ));
                }
                EditMode::OneOfVariantDropdown {
                    options,
                    descriptions,
                    selected,
                    scroll_offset,
                    filter_buffer,
                    filtered_indices,
                    ..
                } if is_selected => {
                    buf.set_string(first_line_val_x, y, "null", value_style);
                    let filtered: Vec<String> = filtered_indices
                        .iter()
                        .map(|&i| options[i].clone())
                        .collect();
                    let filtered_descs: Vec<Option<String>> = filtered_indices
                        .iter()
                        .map(|&i| descriptions[i].clone())
                        .collect();
                    edit_overlay_info = Some((
                        area.x.saturating_add(x_offset).saturating_add(2),
                        y,
                        filtered,
                        filtered_descs,
                        selected,
                        *scroll_offset,
                        filter_buffer.clone(),
                    ));
                }
                _ => {
                    let active_search = match node.value_type {
                        ValueType::Object | ValueType::Array => None,
                        _ => state.search_query.as_deref(),
                    };
                    let placeholder = if (node.value_display == "null"
                        || node.value_display.is_empty())
                        && !matches!(
                            node.node_type,
                            NodeType::Object { .. } | NodeType::Array { .. }
                        ) {
                        state.schema.as_ref().and_then(|s| {
                            find_sub_schema(s, &node.path).and_then(format_type_placeholder)
                        })
                    } else {
                        None
                    };
                    if let Some(ref ph) = placeholder {
                        let mut ph_style = Style::default().fg(Color::DarkGray);
                        if let Some(bg) = item_bg {
                            ph_style = ph_style.bg(bg);
                        }
                        buf.set_string(first_line_val_x, y, ph, ph_style);
                    } else if is_selected && lines_used > 1 {
                        render_wrapped_text(
                            buf,
                            area,
                            y,
                            first_line_val_x,
                            first_line_width,
                            wrapped_val_x,
                            wrapped_line_width,
                            &node.value_display,
                            value_style,
                            None,
                            show_cursor,
                            active_search,
                        );
                    } else {
                        render_highlighted_line(
                            buf,
                            first_line_val_x,
                            y,
                            &node.value_display,
                            first_line_width,
                            value_style,
                            active_search,
                        );
                    }
                }
            }
        }

        current_y += lines_used;
        node_offset += 1;
    }

    let mut active_tip = None;
    if let Some(y) = selected_render_y {
        if let Some(x) = selected_render_x {
            if let Some(node_h) = selected_render_height {
                if let Some(node) = state.flattened_nodes.get(state.selected) {
                    if let Some(schema) = &state.schema {
                        if let Some(sub) = find_sub_schema(schema, &node.path) {
                            if let Some(desc) = extract_description(sub) {
                                let max_tip_width = area
                                    .right()
                                    .saturating_sub(x)
                                    .saturating_sub(2)
                                    .clamp(20, 60);
                                let rect = render_tooltip(
                                    area,
                                    buf,
                                    x,
                                    y + node_h,
                                    max_tip_width,
                                    &desc,
                                    theme,
                                    selected_render_bg,
                                    state,
                                );
                                active_tip = Some(rect);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut active_dropdown = None;
    if let Some((x, y, options, descs, selected, scroll_offset, filter_buffer)) = edit_overlay_info
    {
        let (visible, tip, dropdown) = render_dropdown(
            area,
            buf,
            x,
            y,
            &options,
            &descs,
            *selected,
            scroll_offset,
            &filter_buffer,
            theme,
            state,
        );
        state.dropdown_visible_items = visible;
        active_tip = tip;
        active_dropdown = dropdown;
    }

    (active_tip, active_dropdown)
}

fn render_dropdown(
    area: Rect,
    buf: &mut Buffer,
    x: u16,
    y: u16,
    options: &[String],
    descriptions: &[Option<String>],
    selected: usize,
    scroll_offset: usize,
    _filter_buffer: &str,
    theme: &Theme,
    state: &EditorState,
) -> (usize, Option<Rect>, Option<Rect>) {
    if options.is_empty() {
        return (0, None, None);
    }

    let max_opt_width = options
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.as_str()))
        .max()
        .unwrap_or(0) as u16;
    let width = (max_opt_width + 4).min(area.width);
    let max_height = 12; // borders(2) + max_items(10)
    let height = (options.len() as u16 + 2).min(max_height).min(area.height);

    let mut popup_x = x;
    if popup_x + width > area.right() {
        popup_x = area.right().saturating_sub(width);
    }

    let mut popup_y = y + 1;
    if popup_y + height > area.bottom() {
        popup_y = y.saturating_sub(height);
    }

    if popup_y + height > area.bottom() {
        popup_y = area.bottom().saturating_sub(height);
    }

    let popup_area = Rect::new(popup_x, popup_y, width, height);
    ratatui::widgets::Clear.render(popup_area, buf);

    let block = Block::bordered().border_style(theme.bracket_style);
    block.render(popup_area, buf);

    let visible_items = height.saturating_sub(2) as usize;
    let start = scroll_offset;
    let end = (start + visible_items).min(options.len());

    for (i, opt) in options[start..end].iter().enumerate() {
        let opt_y = popup_y + 1 + i as u16;
        if opt_y < area.bottom() && opt_y < buf.area.bottom() {
            let actual_index = start + i;
            let style = if actual_index == selected {
                theme.focused_style
            } else {
                Style::default()
            };
            let opt_width = (width.saturating_sub(4)) as usize;
            set_string_and_clear(buf, popup_x + 2, opt_y, opt, opt_width, style);
        }
    }

    // Render scrollbar if needed
    if options.len() > visible_items {
        let scrollbar_x = popup_area.right().saturating_sub(1);
        let scrollbar_height = visible_items as u16;
        let max_scroll = (options.len() - visible_items) as u16;
        let thumb_position = if max_scroll > 0 {
            (scroll_offset as u16 * (scrollbar_height.saturating_sub(1))) / max_scroll
        } else {
            0
        };
        for i in 0..scrollbar_height {
            let sy = popup_y + 1 + i;
            if sy < area.bottom() && sy < buf.area.bottom() {
                let ch = if i == thumb_position { "█" } else { "│" };
                buf.set_string(scrollbar_x, sy, ch, theme.bracket_style);
            }
        }
    }

    let mut tip_area = None;
    // Render description tooltip for the selected item using render_tooltip
    if let Some(desc) = descriptions.get(selected).and_then(|d| d.as_ref()) {
        if !desc.is_empty() {
            // Position tooltip to the right of the dropdown
            let mut tip_x = popup_area.right() + 1;
            let mut max_tip_width = area.right().saturating_sub(tip_x);
            if max_tip_width < 10 {
                // No space on right — try left of dropdown
                tip_x = popup_area.x.saturating_sub(10);
                let max_tip_width_left = popup_area.x.saturating_sub(tip_x);
                if max_tip_width_left < 10 {
                    // Still too narrow, use whatever space is available on right
                    tip_x = popup_area.right() + 1;
                }
            }
            max_tip_width = area.right().saturating_sub(tip_x);
            if max_tip_width >= 5 && tip_x < area.right() {
                // Vertical: align with the selected item row
                let sel_row = selected.saturating_sub(scroll_offset) as u16;
                let tip_y = popup_y + 1 + sel_row;

                let rect = render_tooltip(
                    area,
                    buf,
                    tip_x,
                    tip_y,
                    max_tip_width.clamp(20, 60),
                    desc,
                    theme,
                    None,
                    state,
                );
                tip_area = Some(rect);
            }
        }
    }

    (visible_items, tip_area, Some(popup_area))
}

fn render_tooltip(
    area: Rect,
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_tip_width: u16,
    desc: &str,
    theme: &Theme,
    item_bg: Option<Color>,
    state: &EditorState,
) -> Rect {
    let desc_style = Style::default().fg(Color::Rgb(108, 112, 134)); // Overlay1
    let mut desc_style = desc_style;
    if let Some(bg) = item_bg {
        desc_style = desc_style.bg(bg);
    }

    // Text wrapping
    let lines = wrap_text(desc, max_tip_width as usize);
    if lines.is_empty() {
        return Rect::default();
    }

    let total_lines = lines.len();
    let max_tooltip_height = 8usize;
    let scroll_limit = total_lines.saturating_sub(max_tooltip_height);
    let t_scroll = state.tooltip_scroll_offset.min(scroll_limit);
    let display_lines_count = total_lines.min(max_tooltip_height);

    let tip_height = (display_lines_count + 2) as u16; // +2 for borders

    let text_width = lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0) as u16;

    let has_scrollbar = total_lines > max_tooltip_height;
    let scrollbar_width_offset = if has_scrollbar { 2 } else { 0 };
    let tip_width = text_width + 2 + scrollbar_width_offset; // +2 for borders

    // Clamp width to area width
    let tip_width = tip_width.min(area.width);

    // X clamping
    let mut tip_x = x;
    if tip_x + tip_width > area.right() {
        tip_x = area.right().saturating_sub(tip_width);
    }
    if tip_x < area.x {
        tip_x = area.x;
    }

    // Y clamping
    let mut tip_y = y;
    if tip_y + tip_height > area.bottom() {
        tip_y = area.bottom().saturating_sub(tip_height);
    }
    if tip_y < area.y {
        tip_y = area.y;
    }

    let tip_area = Rect::new(tip_x, tip_y, tip_width, tip_height);
    ratatui::widgets::Clear.render(tip_area, buf);

    let mut border_style = theme.bracket_style;
    if let Some(bg) = item_bg {
        border_style = border_style.bg(bg);
    }
    let mut block = Block::bordered().border_style(border_style);
    if has_scrollbar {
        block = block.title(
            Line::from(" ↕ PgUp/PgDn ")
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::Rgb(76, 79, 105))),
        );
    }
    block.render(tip_area, buf);

    // Text rendering
    let display_end = (t_scroll + display_lines_count).min(total_lines);
    let display_lines = &lines[t_scroll..display_end];

    for (i, line) in display_lines.iter().enumerate() {
        let line_y = tip_y + 1 + i as u16;
        if line_y < area.bottom() && line_y < buf.area.bottom() {
            set_string_and_clear(
                buf,
                tip_x + 1,
                line_y,
                line,
                (tip_width
                    .saturating_sub(2)
                    .saturating_sub(scrollbar_width_offset)) as usize,
                desc_style,
            );
        }
    }

    // Scrollbar rendering
    if has_scrollbar {
        let scrollbar_x = tip_x + tip_width - 1;
        let thumb_pos = (t_scroll * (display_lines_count - 1)) / scroll_limit;
        for i in 0..display_lines_count {
            let sy = tip_y + 1 + i as u16;
            if sy < area.bottom() && sy < buf.area.bottom() {
                let ch = if i == thumb_pos { "█" } else { "│" };
                buf.set_string(scrollbar_x, sy, ch, border_style);
            }
        }
    }
    tip_area
}

fn render_wrapped_text(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    first_line_x: u16,
    first_line_width: usize,
    wrapped_x: u16,
    wrapped_width: usize,
    text: &str,
    style: Style,
    cursor_pos: Option<usize>,
    show_cursor: bool,
    search_query: Option<&str>,
) {
    use unicode_width::UnicodeWidthChar;

    let mut row: u16 = 0;
    let mut current_line_width = 0;
    let mut line_start_x = first_line_x;
    let mut line_max_width = first_line_width;

    let mut chars = text.chars().enumerate().peekable();
    let mut current_row_y = y;

    let highlight_style = Style::default()
        .fg(Color::Rgb(17, 17, 27))
        .bg(Color::Rgb(249, 226, 175));

    let mut match_ranges = Vec::new();
    if let Some(query) = search_query {
        if !query.is_empty() {
            let query_lower = query.to_lowercase();
            let text_lower = text.to_lowercase();
            let mut start = 0;
            while let Some(idx) = text_lower[start..].find(&query_lower) {
                let match_start = start + idx;
                let match_end = match_start + query.len();
                match_ranges.push((match_start, match_end));
                start = match_start + 1;
            }
        }
    }

    if text.is_empty() {
        if let Some(0) = cursor_pos {
            if show_cursor && current_row_y < area.bottom() {
                if let Some(cell) = buf.cell_mut((first_line_x, current_row_y)) {
                    cell.set_char(' ')
                        .set_style(style.add_modifier(Modifier::REVERSED));
                }
                // Clear rest of line 0
                for x in
                    (first_line_x + 1)..area.right().min(first_line_x + first_line_width as u16)
                {
                    buf[(x, current_row_y)].set_char(' ').set_style(style);
                }
            } else {
                // No cursor, but still clear the line
                for x in first_line_x..area.right().min(first_line_x + first_line_width as u16) {
                    buf[(x, current_row_y)].set_char(' ').set_style(style);
                }
            }
        } else {
            // No cursor pos 0, just clear
            for x in first_line_x..area.right().min(first_line_x + first_line_width as u16) {
                buf[(x, current_row_y)].set_char(' ').set_style(style);
            }
        }
        return;
    }

    while let Some((i, c)) = chars.next() {
        if current_row_y >= area.bottom() {
            break;
        }

        let c_width = c.width().unwrap_or(0);

        // Wrap if needed
        if current_line_width + c_width > line_max_width {
            // Clear remaining space in current line before wrapping
            for x in (line_start_x + current_line_width as u16)
                ..area.right().min(line_start_x + line_max_width as u16)
            {
                buf[(x, current_row_y)].set_char(' ').set_style(style);
            }

            row += 1;
            current_row_y = y + row;
            if current_row_y >= area.bottom() {
                break;
            }
            line_start_x = wrapped_x;
            line_max_width = wrapped_width;
            current_line_width = 0;
        }

        // Render char
        let cx = line_start_x + current_line_width as u16;
        if cx < area.right() {
            let mut char_style = style;
            if match_ranges.iter().any(|(s, e)| i >= *s && i < *e) {
                char_style = highlight_style;
            }

            let cell = &mut buf[(cx, current_row_y)];
            cell.set_char(c);
            cell.set_style(char_style);

            // Handle cursor
            if let Some(pos) = cursor_pos {
                if pos == i && show_cursor {
                    cell.set_style(char_style.add_modifier(Modifier::REVERSED));
                }
            }
        }

        current_line_width += c_width;

        // If it was the last char
        if chars.peek().is_none() {
            // Handle cursor at the end
            if let Some(pos) = cursor_pos {
                if pos == i + 1 && show_cursor {
                    // Place cursor after last char
                    if current_line_width < line_max_width {
                        let cx = line_start_x + current_line_width as u16;
                        if cx < area.right() {
                            buf[(cx, current_row_y)]
                                .set_char(' ')
                                .set_style(style.add_modifier(Modifier::REVERSED));
                            current_line_width += 1; // Mark as used for clearing logic below
                        }
                    } else {
                        // Wrap cursor to next line
                        // Clear current line first
                        for x in (line_start_x + current_line_width as u16)
                            ..area.right().min(line_start_x + line_max_width as u16)
                        {
                            buf[(x, current_row_y)].set_char(' ').set_style(style);
                        }

                        row += 1;
                        let next_y = y + row;
                        if next_y < area.bottom() {
                            buf[(wrapped_x, next_y)]
                                .set_char(' ')
                                .set_style(style.add_modifier(Modifier::REVERSED));
                            // Also clear rest of this next line
                            for x in
                                (wrapped_x + 1)..area.right().min(wrapped_x + wrapped_width as u16)
                            {
                                buf[(x, next_y)].set_char(' ').set_style(style);
                            }
                        }
                        // We already handled clearing for the current line and the next line if cursor wrapped.
                        return;
                    }
                }
            }

            // Clear remaining space in the current line
            for x in (line_start_x + current_line_width as u16)
                ..area.right().min(line_start_x + line_max_width as u16)
            {
                buf[(x, current_row_y)].set_char(' ').set_style(style);
            }
        }
    }
}

pub fn extract_type_hint(sub_schema: &serde_json::Value) -> String {
    extract_type_hint_for_value(sub_schema, None)
}

/// Value-aware type hint extraction.
/// When value is provided and sub_schema has oneOf/anyOf, shows the matching variant's type.
pub fn extract_type_hint_for_value(
    sub_schema: &serde_json::Value,
    value: Option<&serde_json::Value>,
) -> String {
    if sub_schema.get("enum").is_some() {
        return " [Enum]".to_string();
    }

    if let Some(t) = sub_schema.get("type") {
        if let Some(s) = t.as_str() {
            return format_type_name(s);
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = arr.iter().find_map(|v| {
                let s = v.as_str()?;
                if s != "null" { Some(s) } else { None }
            }) {
                return format_type_name(first);
            }
        }
    }

    // Handle anyOf/oneOf/allOf
    let combo_key = if sub_schema.get("oneOf").is_some() {
        Some("oneOf")
    } else if sub_schema.get("anyOf").is_some() {
        Some("anyOf")
    } else if sub_schema.get("allOf").is_some() {
        Some("allOf")
    } else {
        None
    };

    if let Some(key) = combo_key {
        if let Some(arr) = sub_schema.get(key).and_then(|v| v.as_array()) {
            // If value is non-null, try to find the matching variant
            if let Some(val) = value {
                if !val.is_null() {
                    let target_type = match val {
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Object(_) => "object",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Number(n) => {
                            if n.is_i64() || n.is_u64() {
                                "integer"
                            } else {
                                "number"
                            }
                        }
                        serde_json::Value::Bool(_) => "boolean",
                        serde_json::Value::Null => "null",
                    };
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

fn format_type_name(t: &str) -> String {
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

pub fn extract_description(sub_schema: &serde_json::Value) -> Option<String> {
    sub_schema
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn format_type_placeholder(sub_schema: &serde_json::Value) -> Option<String> {
    if sub_schema.get("enum").is_some() {
        return Some("(enum)".to_string());
    }
    if let Some(t) = sub_schema.get("type") {
        if let Some(s) = t.as_str() {
            return Some(format!("({})", s));
        } else if let Some(arr) = t.as_array() {
            if let Some(first) = arr.iter().find_map(|v| {
                let s = v.as_str()?;
                if s != "null" { Some(s) } else { None }
            }) {
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

pub fn wrap_text(desc: &str, max_width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut remaining = desc;
    while !remaining.is_empty() {
        let mut break_at = remaining.len();
        let mut width = 0;
        let mut last_break = 0;
        for (i, ch) in remaining.char_indices() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + w > max_width {
                break;
            }
            width += w;
            if ch == ' ' {
                last_break = i + ch.len_utf8();
            }
            break_at = i + ch.len_utf8();
        }
        let end = if last_break > 0 && break_at < remaining.len() {
            last_break
        } else {
            break_at
        };
        lines.push(remaining[..end].trim_end().to_string());
        remaining = &remaining[end..];
    }
    lines
}

fn render_highlighted_line(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    width: usize,
    base_style: Style,
    search_query: Option<&str>,
) {
    if let Some(query) = search_query {
        if !query.is_empty() {
            let query_lower = query.to_lowercase();
            let text_lower = text.to_lowercase();

            let highlight_style = Style::default()
                .fg(Color::Rgb(17, 17, 27))
                .bg(Color::Rgb(249, 226, 175));

            let mut current_x = x;
            let mut remaining_width = width;

            let mut start_search_idx = 0;
            let mut last_idx = 0;

            while let Some(idx) = text_lower[start_search_idx..].find(&query_lower) {
                let match_start = start_search_idx + idx;
                let match_end = match_start + query.len();

                // Text before match
                let before = &text[last_idx..match_start];
                let before_width = unicode_width::UnicodeWidthStr::width(before);
                if remaining_width > 0 {
                    buf.set_stringn(current_x, y, before, remaining_width, base_style);
                    current_x += before_width as u16;
                    remaining_width = remaining_width.saturating_sub(before_width);
                }

                // Match
                let matched = &text[match_start..match_end];
                let matched_width = unicode_width::UnicodeWidthStr::width(matched);
                if remaining_width > 0 {
                    buf.set_stringn(current_x, y, matched, remaining_width, highlight_style);
                    current_x += matched_width as u16;
                    remaining_width = remaining_width.saturating_sub(matched_width);
                }

                last_idx = match_end;
                start_search_idx = match_end;
                if start_search_idx >= text.len() || remaining_width == 0 {
                    break;
                }
            }

            // Text after last match
            if last_idx < text.len() && remaining_width > 0 {
                let after = &text[last_idx..];
                buf.set_stringn(current_x, y, after, remaining_width, base_style);
                current_x += unicode_width::UnicodeWidthStr::width(after) as u16;
            }

            // Clear remaining width
            if (current_x as usize) < (x as usize + width) {
                for i in current_x..(x + width as u16) {
                    if i < buf.area.right() {
                        buf[(i, y)].set_char(' ').set_style(base_style);
                    }
                }
            }
            return;
        }
    }
    set_string_and_clear(buf, x, y, text, width, base_style);
}

fn set_string_and_clear(buf: &mut Buffer, x: u16, y: u16, text: &str, width: usize, style: Style) {
    buf.set_stringn(x, y, text, width, style);
    let text_width = unicode_width::UnicodeWidthStr::width(text);
    if text_width < width {
        for i in (x + text_width as u16)..(x + width as u16) {
            if i < buf.area.right() {
                buf[(i, y)].set_char(' ').set_style(style);
            }
        }
    }
}

fn render_status_bar(
    area: Rect,
    buf: &mut Buffer,
    state: &EditorState,
    theme: &Theme,
    show_cursor: bool,
) {
    // Clear entire status bar area first
    for x in area.x..area.right() {
        buf[(x, area.y)].set_char(' ').set_style(theme.status_style);
    }

    if let EditMode::SearchPrompt { buffer, cursor_pos } = &state.edit_mode {
        let prompt_prefix = if state.search_total_matches > 0 {
            format!(
                " Search [ {}/{} ]: ",
                state.search_current_match_index, state.search_total_matches
            )
        } else {
            " Search: ".to_string()
        };
        let prompt = format!("{}{}", prompt_prefix, buffer);
        buf.set_string(area.x, area.y, &prompt, theme.status_style);

        // Render Esc help hint at right end
        let esc_hint = " Esc: Exit ";
        if area.width > 40 && area.width > esc_hint.len() as u16 + 2 {
            let hint_style = theme.status_style.fg(Color::Rgb(76, 79, 105));
            buf.set_string(
                area.x + area.width - esc_hint.len() as u16 - 1,
                area.y,
                esc_hint,
                hint_style,
            );
        }

        // Render cursor in search prompt (blinking)
        let prefix = &buffer[..crate::state::EditorState::char_to_byte_index(buffer, *cursor_pos)];
        let prompt_prefix_len =
            unicode_width::UnicodeWidthStr::width(prompt_prefix.as_str()) as u16;
        let cursor_x =
            area.x + prompt_prefix_len + unicode_width::UnicodeWidthStr::width(prefix) as u16;
        if cursor_x < area.x + area.width {
            if show_cursor {
                let char_count = buffer.chars().count();
                let char_to_invert = if *cursor_pos < char_count {
                    buffer.chars().nth(*cursor_pos).unwrap_or(' ')
                } else {
                    ' '
                };
                buf[(cursor_x, area.y)]
                    .set_char(char_to_invert)
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
        return;
    }

    let schema_status = match &state.schema_state {
        crate::state::SchemaState::None => "".to_string(),
        crate::state::SchemaState::Loading => " [Schema: Loading...] ".to_string(),
        crate::state::SchemaState::Loaded => "".to_string(),
        crate::state::SchemaState::Error(e) => format!(" [Schema: Error! {}] ", e),
    };

    let path_info = if let Some(node) = state.selected_node() {
        format!(" {}: {} ", state.selected + 1, node.path.join("/"))
    } else {
        format!(" {} ", state.selected + 1)
    };

    let message = if let Some((msg, time)) = &state.status_message {
        if time.elapsed() < std::time::Duration::from_secs(3) {
            format!(" | {} ", msg)
        } else {
            "".to_string()
        }
    } else {
        "".to_string()
    };

    let text = format!("{}{}{}", schema_status, path_info, message);
    buf.set_string(area.x, area.y, &text, theme.status_style);

    // Render Help hint or Search info at right end when not searching
    let mut right_text = " Help: ? ".to_string();
    if state.search_query.is_some() && state.search_total_matches > 0 {
        right_text = format!(
            " [ {}/{} ] Esc: Clear Search ",
            state.search_current_match_index, state.search_total_matches
        );
    }

    if area.width > 40 && area.width > right_text.len() as u16 + 2 {
        let hint_style = theme.status_style.fg(Color::Rgb(76, 79, 105));
        buf.set_string(
            area.x + area.width - right_text.len() as u16 - 1,
            area.y,
            &right_text,
            hint_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn test_render_wrapped_text_ghost_characters() {
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);

        // Fill first line with 'A'
        for x in 0..10 {
            buf[(x, 0)].set_symbol("A");
        }

        // Render "한글" (width 4) into the first line
        render_wrapped_text(
            &mut buf,
            area,
            0,  // y
            0,  // first_line_x
            10, // first_line_width
            0,  // wrapped_x
            10, // wrapped_width
            "한글",
            Style::default(),
            None,
            false,
            None,
        );

        assert_eq!(buf[(0, 0)].symbol(), "한");
        assert_eq!(buf[(2, 0)].symbol(), "글");

        // Index 4 should be empty (' ') as it should be cleared now.
        assert_eq!(
            buf[(4, 0)].symbol(),
            " ",
            "Ghost character should be cleared!"
        );
    }

    #[test]
    fn test_render_status_bar_ghost_characters() {
        use crate::state::{EditMode, EditorState};
        use crate::theme::Theme;

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let theme = Theme::default();
        let mut state = EditorState::new(
            serde_json::json!({}),
            crate::format::Format::Json,
            None,
            None,
        );

        // Fill with 'A'
        for x in 0..20 {
            buf[(x, 0)].set_symbol("A");
        }

        // Set search prompt with short text
        state.edit_mode = EditMode::SearchPrompt {
            buffer: "abc".to_string(),
            cursor_pos: 3,
        };

        render_status_bar(area, &mut buf, &state, &theme, false);

        // " Search: abc" is 12 chars. Index 12 should be cleared.
        assert_eq!(buf[(12, 0)].symbol(), " ");
        assert_eq!(buf[(19, 0)].symbol(), " ");
    }

    #[test]
    fn test_render_status_bar_search_info_in_normal_mode() {
        use crate::state::{EditMode, EditorState};
        use crate::theme::Theme;

        let area = Rect::new(0, 0, 50, 1);
        let mut buf = Buffer::empty(area);
        let theme = Theme::default();
        let mut state = EditorState::new(
            serde_json::json!({"a": 1}),
            crate::format::Format::Json,
            None,
            None,
        );

        // Setup active search in Normal mode
        state.search_query = Some("a".to_string());
        state.search_total_matches = 5;
        state.search_current_match_index = 2;
        state.edit_mode = EditMode::Normal;

        render_status_bar(area, &mut buf, &state, &theme, false);

        // Check if "[ 2/5 ]" or similar exists in the buffer
        let mut found = false;
        for x in 0..area.width {
            let s = buf[(x, 0)].symbol();
            if s == "[" {
                // Check if following contains "2/5"
                let mut combined = String::new();
                for i in 0..7 {
                    if x + i < area.width {
                        combined.push_str(buf[(x + i, 0)].symbol());
                    }
                }
                if combined.contains("2/5") {
                    found = true;
                    break;
                }
            }
        }
        assert!(
            found,
            "Search match info [2/5] should be rendered in Normal mode"
        );
    }
}
