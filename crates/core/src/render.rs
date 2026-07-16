use crate::format::Format;
use crate::schema_util::{
    extract_description, extract_type_hint_for_value, find_sub_schema, format_type_placeholder,
};
use crate::state::{EditMode, EditorState, NodeType, ValueType};
use crate::theme::Theme;
use crate::tooltip;
use crate::util::char_to_byte_index;
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
        state.tooltip.area = None;
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
        state.tooltip.area = tip_area;
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

        if matches!(state.edit_mode, EditMode::Help { .. }) {
            render_help_modal(area, buf, state, self.theme);
        }
    }
}

fn render_help_modal(area: Rect, buf: &mut Buffer, state: &mut EditorState, theme: &Theme) {
    let backdrop_style = Style::default()
        .bg(Color::Rgb(17, 17, 27))
        .fg(Color::Rgb(88, 91, 112));
    buf.set_style(area, backdrop_style);

    #[derive(Clone, Copy)]
    enum Row {
        Title(&'static str),
        Entry(&'static str, &'static str),
    }
    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "NAVIGATION",
            &[
                ("Up/Down", "Navigate through nodes"),
                ("Left", "Collapse node"),
                ("Right", "Expand node / Move to last child"),
                ("Space", "Toggle expansion"),
                ("Ctrl+Up/Dn", "Jump to prev/next sibling"),
                ("Alt+Up/Dn", "Move node up/down (reorder)"),
                ("PgUp/PgDn", "Scroll page up/down"),
            ],
        ),
        (
            "EDITING",
            &[
                ("Enter", "Add child (Obj/Arr) or Edit leaf"),
                ("Backspace", "Edit and clear current value"),
                ("Delete/D", "Delete selected node"),
                ("S", "Save changes"),
                ("/", "Search key/value"),
            ],
        ),
        (
            "VIEW",
            &[("T", "Toggle type hints"), ("K", "Toggle child counts")],
        ),
        (
            "SESSION",
            &[
                ("U", "Undo"),
                ("R", "Redo"),
                ("?", "Show / hide this help"),
                ("Esc or Q", "Quit (prompts to save if modified)"),
                ("Ctrl+C", "Force quit without saving"),
            ],
        ),
    ];

    let mut rows: Vec<Row> = Vec::new();
    for (title, entries) in sections {
        rows.push(Row::Title(title));
        for (k, d) in *entries {
            rows.push(Row::Entry(k, d));
        }
        rows.push(Row::Title("")); // blank spacer line
    }

    let total_lines = rows.len();
    let width = 64.min(area.width).max(40);
    let height = ((total_lines + 4) as u16).min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    ratatui::widgets::Clear.render(popup_area, buf);

    let content_x = popup_area.x + 4;
    let body_top = popup_area.y + 2;
    let footer_y = popup_area.y + height - 2;
    let visible = (height as usize).saturating_sub(4).max(1);

    let max_offset = total_lines.saturating_sub(visible);
    if let EditMode::Help { max_offset: m, .. } = &mut state.edit_mode {
        *m = max_offset;
    }
    let offset = match &state.edit_mode {
        EditMode::Help { scroll_offset, .. } => (*scroll_offset).min(max_offset),
        _ => 0,
    };

    for (i, row) in rows.iter().enumerate().skip(offset).take(visible) {
        let line_y = body_top + (i - offset) as u16;
        match row {
            Row::Title(t) => {
                if !t.is_empty() {
                    buf.set_string(content_x, line_y, *t, theme.focused_style);
                }
            }
            Row::Entry(k, d) => {
                buf.set_string(content_x, line_y, *k, theme.key_style);
                buf.set_string(content_x + 14, line_y, *d, theme.string_style);
            }
        }
    }

    let scroll_info = if max_offset > 0 {
        format!(" · {}/{}", offset + 1, max_offset + 1)
    } else {
        String::new()
    };
    let footer = format!("PgUp/PgDn scroll · any key close{}", scroll_info);
    buf.set_string(
        x + (width.saturating_sub(footer.len() as u16)) / 2,
        footer_y,
        &footer,
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

    let backdrop_style = Style::default()
        .bg(Color::Rgb(17, 17, 27))
        .fg(Color::Rgb(88, 91, 112));
    buf.set_style(area, backdrop_style);

    ratatui::widgets::Clear.render(popup_area, buf);

    let msg = "Save changes before exiting?";
    let no_text = "[N]o";
    let yes_text = "[Y]es";

    buf.set_string(
        x + (width - msg.len() as u16) / 2,
        y + 2,
        msg,
        Style::default().add_modifier(Modifier::BOLD),
    );

    // Spaced out buttons: "[N]o    [Y]es"
    let buttons_width = no_text.len() as u16 + 4 + yes_text.len() as u16;
    let buttons_x = x + (width - buttons_width) / 2;
    let buttons_y = y + 4;
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

fn render_text_prompt_value(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    first_line_val_x: u16,
    first_line_width: usize,
    wrapped_val_x: u16,
    wrapped_line_width: usize,
    buffer: &str,
    cursor_pos: usize,
    value_style: Style,
    show_cursor: bool,
    item_bg: Option<Color>,
    state: &EditorState,
    node_path: &[String],
) {
    if buffer.is_empty() {
        let placeholder = state
            .schema
            .as_ref()
            .and_then(|s| find_sub_schema(s, node_path).and_then(format_type_placeholder));
        if let Some(ref ph) = placeholder {
            let mut ph_style = Style::default().fg(Color::DarkGray);
            if let Some(bg) = item_bg {
                ph_style = ph_style.bg(bg);
            }
            buf.set_string(first_line_val_x, y, ph, ph_style);
            if show_cursor && cursor_pos == 0 {
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
                Some(cursor_pos),
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
            Some(cursor_pos),
            show_cursor,
            state.search_query.as_deref(),
        );
    }
}

type DropdownOverlayInfo = Option<(
    u16,
    u16,
    Vec<String>,
    Vec<Option<String>>,
    usize,
    usize,
    String,
)>;

fn render_dropdown_value(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    first_line_val_x: u16,
    first_line_width: usize,
    wrapped_val_x: u16,
    wrapped_line_width: usize,
    options: &[String],
    descriptions: &[Option<String>],
    selected: usize,
    scroll_offset: &usize,
    filter_buffer: &str,
    filtered_indices: &[usize],
    value_style: Style,
    show_cursor: bool,
    state: &EditorState,
    value_display: &str,
) -> DropdownOverlayInfo {
    let display_text = if filter_buffer.is_empty() {
        value_display
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
    Some((
        first_line_val_x,
        y,
        filtered,
        filtered_descs,
        selected,
        *scroll_offset,
        filter_buffer.to_string(),
    ))
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
                    let node_value = state.node_at_path_as_value(&node.path);
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

        if node.depth > 0 {
            if let Some(schema) = &state.schema {
                if let Some(sub) = find_sub_schema(schema, &node.path) {
                    if let Some(desc) = extract_description(sub) {
                        let node_x = area.x.saturating_add(x_offset);
                        let max_tip_width = area
                            .right()
                            .saturating_sub(node_x)
                            .saturating_sub(2)
                            .clamp(20, 60);
                        let tip_lines =
                            crate::tooltip::count_markdown_lines(&desc, max_tip_width as usize);
                        let display_lines = tip_lines.min(8);
                        selected_lines += (display_lines + 2) as u16;
                    }
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

        let prefix = if node.is_disabled_comment {
            match state.format {
                Format::Jsonc => "// ",
                _ => "# ",
            }
        } else {
            match node.node_type {
                NodeType::Object { .. } | NodeType::Array { .. } => {
                    if node.expanded {
                        "▼ "
                    } else {
                        "▶ "
                    }
                }
                NodeType::Leaf => "  ",
            }
        };

        let is_hovered = state.hovered_node == Some(node_offset);
        let is_modified = {
            let base_modified = state.is_node_modified(&node.path);
            if is_selected {
                match &state.edit_mode {
                    EditMode::TextPrompt { buffer, .. } => {
                        let orig_val =
                            crate::node::find_node_by_path(&state.original_nodes, &node.path)
                                .and_then(|i| state.original_nodes.get(i))
                                .map(|n| &n.value);
                        if let Some(orig_val) = orig_val {
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
        } else if node.is_disabled_comment {
            theme.disabled_style
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
        } else if node.is_disabled_comment {
            theme.disabled_style
        } else {
            theme.key_style
        };
        if let Some(bg) = item_bg {
            key_style = key_style.bg(bg);
        }

        let mut value_style = if is_selected {
            theme.focused_style
        } else if node.is_disabled_comment {
            theme.disabled_style
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
                    let node_value = state.node_at_path_as_value(&node.path);
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
                    render_text_prompt_value(
                        buf,
                        area,
                        y,
                        first_line_val_x,
                        first_line_width,
                        wrapped_val_x,
                        wrapped_line_width,
                        buffer,
                        *cursor_pos,
                        value_style,
                        show_cursor,
                        item_bg,
                        state,
                        &node.path,
                    );
                }
                EditMode::Dropdown {
                    options,
                    descriptions,
                    selected,
                    scroll_offset,
                    filter_buffer,
                    filtered_indices,
                } if is_selected => {
                    edit_overlay_info = render_dropdown_value(
                        buf,
                        area,
                        y,
                        first_line_val_x,
                        first_line_width,
                        wrapped_val_x,
                        wrapped_line_width,
                        options,
                        descriptions,
                        *selected,
                        scroll_offset,
                        filter_buffer,
                        filtered_indices,
                        value_style,
                        show_cursor,
                        state,
                        &node.value_display,
                    );
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
                        *selected,
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
                        *selected,
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
                        )
                        && !node.is_disabled_comment
                    {
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

        // Render comment indicator + preview if node has comments (and not a disabled comment node itself)
        if node.has_comment && !node.is_disabled_comment {
            if let Some(ref preview) = node.comment_preview {
                let indicator = " 💬 ";
                let preview_text = format!("{}{}", indicator, preview);

                // Calculate position after value
                let value_width =
                    unicode_width::UnicodeWidthStr::width(node.value_display.as_str()) as u16;
                let indicator_x = first_line_val_x
                    .saturating_add(value_width)
                    .saturating_add(1);

                if indicator_x < area.right() {
                    let mut indicator_style = theme.comment_indicator_style;
                    if is_selected {
                        indicator_style =
                            indicator_style.bg(theme.focused_style.bg.unwrap_or(Color::Reset));
                    }
                    if let Some(bg) = item_bg {
                        indicator_style = indicator_style.bg(bg);
                    }

                    // Truncate preview to fit
                    let max_preview_width = area.right().saturating_sub(indicator_x) as usize;
                    let preview_width =
                        unicode_width::UnicodeWidthStr::width(preview_text.as_str());
                    let truncated = if preview_width > max_preview_width {
                        // Find safe cut point by character
                        let mut width = 0;
                        let mut cut_idx = 0;
                        for (idx, ch) in preview_text.char_indices() {
                            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                            if width + ch_width + 3 > max_preview_width {
                                break;
                            }
                            width += ch_width;
                            cut_idx = idx + ch.len_utf8();
                        }
                        format!("{}...", &preview_text[..cut_idx])
                    } else {
                        preview_text
                    };

                    buf.set_string(indicator_x, y, &truncated, indicator_style);
                }
            }
        }

        current_y += lines_used;
        node_offset += 1;
    }

    let mut active_tip = match (selected_render_y, selected_render_x, selected_render_height) {
        (Some(y), Some(x), Some(node_h)) => tooltip::render_tooltip_if_available(
            state,
            x,
            y,
            node_h,
            area,
            buf,
            theme,
            selected_render_bg,
        ),
        _ => None,
    };

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
            selected,
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
    state: &mut EditorState,
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

    let tip_area = tooltip::render_dropdown_tip(
        area,
        buf,
        popup_area,
        popup_y,
        selected,
        scroll_offset,
        descriptions,
        theme,
        state,
    );

    (visible_items, tip_area, Some(popup_area))
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
        let has_cursor = cursor_pos == Some(0) && show_cursor && current_row_y < area.bottom();
        if has_cursor {
            if let Some(cell) = buf.cell_mut((first_line_x, current_row_y)) {
                cell.set_char(' ')
                    .set_style(style.add_modifier(Modifier::REVERSED));
            }
            for x in (first_line_x + 1)..area.right().min(first_line_x + first_line_width as u16) {
                buf[(x, current_row_y)].set_char(' ').set_style(style);
            }
        } else {
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
            if handle_end_of_text_cursor(
                buf,
                area,
                y,
                &mut row,
                wrapped_x,
                wrapped_width,
                line_start_x,
                line_max_width,
                current_line_width,
                current_row_y,
                cursor_pos,
                show_cursor,
                i,
                style,
            ) {
                return;
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

/// Handle cursor placement at end of text. Returns true if caller should return early.
fn handle_end_of_text_cursor(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    row: &mut u16,
    wrapped_x: u16,
    wrapped_width: usize,
    line_start_x: u16,
    line_max_width: usize,
    current_line_width: usize,
    current_row_y: u16,
    cursor_pos: Option<usize>,
    show_cursor: bool,
    char_index: usize,
    style: Style,
) -> bool {
    let _ = match cursor_pos {
        Some(p) if p == char_index + 1 && show_cursor => p,
        _ => return false,
    };
    if current_line_width < line_max_width {
        let cx = line_start_x + current_line_width as u16;
        if cx < area.right() {
            buf[(cx, current_row_y)]
                .set_char(' ')
                .set_style(style.add_modifier(Modifier::REVERSED));
        }
        return true;
    }

    // Wrap cursor to next line: clear current line first
    for x in (line_start_x + current_line_width as u16)
        ..area.right().min(line_start_x + line_max_width as u16)
    {
        buf[(x, current_row_y)].set_char(' ').set_style(style);
    }

    *row += 1;
    let next_y = y + *row;
    if next_y < area.bottom() {
        buf[(wrapped_x, next_y)]
            .set_char(' ')
            .set_style(style.add_modifier(Modifier::REVERSED));
        for x in (wrapped_x + 1)..area.right().min(wrapped_x + wrapped_width as u16) {
            buf[(x, next_y)].set_char(' ').set_style(style);
        }
    }
    true
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
    let Some(query) = search_query else {
        set_string_and_clear(buf, x, y, text, width, base_style);
        return;
    };
    if query.is_empty() {
        set_string_and_clear(buf, x, y, text, width, base_style);
        return;
    }
    render_highlighted_inner(buf, x, y, text, width, base_style, query);
}

fn render_highlighted_inner(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    width: usize,
    base_style: Style,
    query: &str,
) {
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
}

pub(crate) fn set_string_and_clear(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    text: &str,
    width: usize,
    style: Style,
) {
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
        let prefix = &buffer[..char_to_byte_index(buffer, *cursor_pos)];
        let prompt_prefix_len =
            unicode_width::UnicodeWidthStr::width(prompt_prefix.as_str()) as u16;
        let cursor_x =
            area.x + prompt_prefix_len + unicode_width::UnicodeWidthStr::width(prefix) as u16;
        if cursor_x < area.x + area.width && show_cursor {
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
