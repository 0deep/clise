use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Paragraph, Widget, Wrap},
};

use crate::{
    state::{EditMode, EditorState},
    theme::Theme,
};

/// Tooltip state management
#[derive(Default)]
pub struct TooltipState {
    pub scroll_offset: usize,
    pub area: Option<Rect>,
    pub max_width: Option<usize>,
}

impl TooltipState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_scroll(&mut self) {
        self.scroll_offset = 0;
    }

    /// Check if tooltip should be displayed based on current mode
    pub fn is_active(&self, state: &EditorState) -> bool {
        match &state.edit_mode {
            EditMode::Normal => {
                if let Some(node) = state.selected_node() {
                    if node.depth > 0 {
                        if let Some(schema) = &state.schema {
                            if let Some(sub) =
                                crate::schema_util::find_sub_schema(schema, &node.path)
                            {
                                if let Some(desc) = crate::schema_util::extract_description(sub) {
                                    return !desc.is_empty();
                                }
                            }
                        }
                    }
                }
                false
            }
            EditMode::Dropdown {
                descriptions,
                selected,
                ..
            }
            | EditMode::NewKeyDropdown {
                descriptions,
                selected,
                ..
            }
            | EditMode::OneOfVariantDropdown {
                descriptions,
                selected,
                ..
            } => {
                if let Some(Some(desc)) = descriptions.get(*selected) {
                    !desc.is_empty()
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Calculate scroll limit for current tooltip based on mode and content
    pub fn scroll_limit(&self, state: &EditorState) -> Option<usize> {
        let desc = match &state.edit_mode {
            EditMode::Normal => {
                let node = state.selected_node()?;
                if node.depth == 0 {
                    return None;
                }
                let schema = state.schema.as_ref()?;
                let sub = crate::schema_util::find_sub_schema(schema, &node.path)?;
                crate::schema_util::extract_description(sub)?
            }
            EditMode::Dropdown {
                descriptions,
                selected,
                ..
            }
            | EditMode::NewKeyDropdown {
                descriptions,
                selected,
                ..
            }
            | EditMode::OneOfVariantDropdown {
                descriptions,
                selected,
                ..
            } => descriptions.get(*selected)?.as_ref()?.clone(),
            _ => return None,
        };

        if desc.is_empty() {
            return None;
        }

        let max_width = self.max_width.unwrap_or(40);
        let text = tui_markdown::from_str(&desc);
        let total_lines = count_wrapped_lines(&text, max_width);
        let max_tooltip_height = 8usize;
        Some(total_lines.saturating_sub(max_tooltip_height))
    }

    /// Apply scroll delta with clamping to the given limit
    pub fn scroll(&mut self, delta: isize) {
        if delta < 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll_offset = self.scroll_offset.saturating_add(delta as usize);
        }
    }

    /// Clamp scroll offset to the given limit
    pub fn clamp_scroll(&mut self, limit: usize) {
        self.scroll_offset = self.scroll_offset.min(limit);
    }
}

/// Count total rendered lines from markdown description after parsing and wrapping.
/// Used by layout code to reserve space for tooltip.
pub(crate) fn count_markdown_lines(desc: &str, max_width: usize) -> usize {
    if max_width == 0 {
        return 0;
    }
    let text = tui_markdown::from_str(desc);
    count_wrapped_lines(&text, max_width)
}

/// Count total rendered lines from a Text widget after word-wrapping at given width.
fn count_wrapped_lines(text: &ratatui::text::Text<'_>, max_width: usize) -> usize {
    if max_width == 0 {
        return text.lines.len();
    }
    let mut total = 0;
    for line in &text.lines {
        let line_width: usize = line.width();
        if line_width == 0 {
            total += 1; // empty line
        } else {
            // ceil(line_width / max_width)
            total += (line_width + max_width - 1) / max_width;
        }
    }
    total
}

// ============================================================================
// Render functions
// ============================================================================

/// Render tooltip for schema description (Normal mode)
pub fn render_tooltip_if_available(
    state: &mut EditorState,
    x: u16,
    y: u16,
    node_h: u16,
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    selected_render_bg: Option<Color>,
) -> Option<Rect> {
    let node = state.flattened_nodes.get(state.selected)?;
    if node.depth == 0 {
        return None;
    }
    let schema = state.schema.as_ref()?;
    let sub = crate::schema_util::find_sub_schema(schema, &node.path)?;
    let desc = crate::schema_util::extract_description(sub)?;
    let max_tip_width = area
        .right()
        .saturating_sub(x)
        .saturating_sub(2)
        .clamp(20, 60);
    state.tooltip.max_width = Some(max_tip_width as usize);
    Some(render_tooltip(
        area,
        buf,
        x,
        y + node_h,
        max_tip_width,
        &desc,
        theme,
        selected_render_bg,
        state,
    ))
}

/// Render tooltip for dropdown item description
pub fn render_dropdown_tip(
    area: Rect,
    buf: &mut Buffer,
    popup_area: Rect,
    popup_y: u16,
    selected: usize,
    scroll_offset: usize,
    descriptions: &[Option<String>],
    theme: &Theme,
    state: &mut EditorState,
) -> Option<Rect> {
    let desc = descriptions.get(selected).and_then(|d| d.as_ref())?;
    if desc.is_empty() {
        return None;
    }

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
    if max_tip_width < 5 || tip_x >= area.right() {
        return None;
    }

    // Vertical: align with the selected item row
    let sel_row = selected.saturating_sub(scroll_offset) as u16;
    let tip_y = popup_y + 1 + sel_row;

    let final_width = max_tip_width.clamp(20, 60);
    state.tooltip.max_width = Some(final_width as usize);
    let rect = render_tooltip(
        area,
        buf,
        tip_x,
        tip_y,
        final_width,
        desc,
        theme,
        None,
        state,
    );
    Some(rect)
}

/// Core tooltip rendering: border + wrap + scroll + scrollbar
pub fn render_tooltip(
    area: Rect,
    buf: &mut Buffer,
    x: u16,
    y: u16,
    max_tip_width: u16,
    desc: &str,
    theme: &Theme,
    item_bg: Option<Color>,
    state: &mut EditorState,
) -> Rect {
    // Parse markdown to styled Text
    let text = tui_markdown::from_str(desc);
    if text.lines.is_empty() {
        return Rect::default();
    }

    let max_width = max_tip_width as usize;
    let total_lines = count_wrapped_lines(&text, max_width);
    let max_tooltip_height = 8usize;
    let scroll_limit = total_lines.saturating_sub(max_tooltip_height);

    // Clamp state offset to actual scroll limit
    state.tooltip.scroll_offset = state.tooltip.scroll_offset.min(scroll_limit);
    let t_scroll = state.tooltip.scroll_offset;
    let display_lines_count = total_lines.min(max_tooltip_height);

    let has_scrollbar = total_lines > max_tooltip_height;
    let scrollbar_width_offset = if has_scrollbar { 2 } else { 0 };

    // Estimate tip width from content
    let text_width = text.lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let text_width = text_width.min(max_tip_width);
    let tip_width = (text_width + 2 + scrollbar_width_offset).min(area.width);

    let tip_height = (display_lines_count + 2) as u16; // +2 for borders

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

    // Border
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

    // Apply background to block
    if let Some(bg) = item_bg {
        block = block.style(Style::default().bg(bg));
    }

    // Render paragraph with markdown text, wrapping, and scroll
    let inner_area = block.inner(tip_area);
    block.render(tip_area, buf);

    let paragraph = Paragraph::new(text)
        .wrap(Wrap { trim: true })
        .scroll((t_scroll as u16, 0));
    paragraph.render(inner_area, buf);

    // Scrollbar rendering
    if has_scrollbar {
        let scrollbar_x = tip_x + tip_width - 1;
        let thumb_pos = if scroll_limit > 0 {
            (t_scroll * (display_lines_count - 1)) / scroll_limit
        } else {
            0
        };
        for i in 0..display_lines_count {
            let sy = tip_y + 1 + i as u16;
            if sy < area.bottom() && sy < buf.area.bottom() {
                let sb = &theme.tooltip_scrollbar;
                let ch = if i == thumb_pos {
                    sb.thumb_symbol
                } else {
                    sb.track_symbol
                };
                buf.set_string(scrollbar_x, sy, ch, sb.style);
            }
        }
    }
    tip_area
}

#[cfg(test)]
mod tests {
    use crate::state::{EditMode, EditorState};
    use serde_json::json;

    #[cfg(feature = "schema")]
    #[test]
    fn test_tooltip_scroll_offset() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "A long description that will require scrolling to view completely"
                }
            }
        });
        let mut state = EditorState::new(
            json!({"name": "test"}),
            crate::format::Format::Json,
            None,
            None,
        );
        state.schema = Some(schema);
        state.selected = 1;
        state.tooltip.max_width = Some(40);

        assert_eq!(state.tooltip.scroll_offset, 0);

        state.tooltip.scroll(1);
        assert_eq!(state.tooltip.scroll_offset, 1);

        state.tooltip.scroll(5);
        assert_eq!(state.tooltip.scroll_offset, 6);

        state.tooltip.scroll(-2);
        assert_eq!(state.tooltip.scroll_offset, 4);

        state.tooltip.scroll(-10);
        assert_eq!(state.tooltip.scroll_offset, 0); // saturating
    }

    #[cfg(feature = "schema")]
    #[test]
    fn test_scroll_tooltip_clamp() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "word ".repeat(200)
                }
            }
        });
        let mut state = EditorState::new(
            json!({"name": "test"}),
            crate::format::Format::Json,
            None,
            None,
        );
        state.schema = Some(schema);
        state.selected = 1;
        state.tooltip.max_width = Some(40);

        assert!(state.tooltip.is_active(&state));

        // "word ".repeat(200) = 1000 chars, wraps to 25 lines at width 40, scroll_limit = 25 - 8 = 17
        let scroll_limit =
            crate::tooltip::count_markdown_lines(&"word ".repeat(200), 40).saturating_sub(8);
        assert_eq!(scroll_limit, 17);

        state.tooltip.scroll(100);
        state.tooltip.clamp_scroll(scroll_limit);
        assert_eq!(state.tooltip.scroll_offset, scroll_limit);

        state.tooltip.scroll(10);
        state.tooltip.clamp_scroll(scroll_limit);
        assert_eq!(state.tooltip.scroll_offset, scroll_limit);

        state.tooltip.scroll(-1);
        assert_eq!(state.tooltip.scroll_offset, scroll_limit - 1);

        state.tooltip.scroll(-100);
        assert_eq!(state.tooltip.scroll_offset, 0);
    }

    #[test]
    fn test_dropdown_tooltip_scroll_clamp() {
        let mut state = EditorState::new(json!("a"), crate::format::Format::Json, None, None);
        state.edit_mode = EditMode::Dropdown {
            options: vec!["a".to_string(), "b".to_string()],
            descriptions: vec![Some("word ".repeat(200)), None],
            selected: 0,
            scroll_offset: 0,
            filter_buffer: String::new(),
            filtered_indices: vec![0, 1],
        };
        state.tooltip.max_width = Some(40);

        assert!(state.tooltip.is_active(&state));

        let limit = state.tooltip.scroll_limit(&state).unwrap();
        assert_eq!(limit, 17);

        state.tooltip.scroll(100);
        state.tooltip.clamp_scroll(limit);
        assert_eq!(state.tooltip.scroll_offset, limit);

        state.tooltip.scroll(10);
        state.tooltip.clamp_scroll(limit);
        assert_eq!(state.tooltip.scroll_offset, limit);

        state.tooltip.scroll(-100);
        assert_eq!(state.tooltip.scroll_offset, 0);
    }

    #[cfg(feature = "schema")]
    #[test]
    fn test_tooltip_scroll_limit_dynamic_width() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "This is a very long description that will wrap differently at different widths"
                }
            }
        });
        let mut state = EditorState::new(
            json!({"name": "test"}),
            crate::format::Format::Json,
            None,
            None,
        );
        state.schema = Some(schema);
        state.selected = 1;

        let limit_default = state.tooltip.scroll_limit(&state).unwrap();

        state.tooltip.max_width = Some(20);
        let limit_narrow = state.tooltip.scroll_limit(&state).unwrap();

        assert!(limit_narrow >= limit_default);
    }
}
