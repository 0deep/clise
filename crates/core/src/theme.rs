use ratatui::style::{Color, Style};

#[derive(Debug, Clone)]
pub struct ScrollbarStyle {
    pub track_symbol: &'static str,
    pub thumb_symbol: &'static str,
    pub style: Style,
    pub begin_symbol: &'static str,
    pub end_symbol: &'static str,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub key_style: Style,
    pub string_style: Style,
    pub number_style: Style,
    pub bool_style: Style,
    pub null_style: Style,
    pub bracket_style: Style,
    pub focused_style: Style,
    pub status_style: Style,
    pub indent_guide_style: Style,
    pub error_style: Style,
    pub disabled_style: Style,
    pub comment_indicator_style: Style,
    pub depth_key_styles: Vec<Style>,
    pub depth_bracket_styles: Vec<Style>,
    pub scrollbar: ScrollbarStyle,
    pub dropdown_scrollbar: ScrollbarStyle,
    pub tooltip_scrollbar: ScrollbarStyle,
}

impl Theme {
    pub fn key_style_for_depth(&self, depth: usize) -> Style {
        if self.depth_key_styles.is_empty() {
            self.key_style
        } else {
            self.depth_key_styles[depth % self.depth_key_styles.len()]
        }
    }

    pub fn bracket_style_for_depth(&self, depth: usize) -> Style {
        if self.depth_bracket_styles.is_empty() {
            self.bracket_style
        } else {
            self.depth_bracket_styles[depth % self.depth_bracket_styles.len()]
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        // Catppuccin Mocha inspired palette
        Self {
            key_style: Style::default().fg(Color::Rgb(137, 180, 250)), // Blue
            string_style: Style::default().fg(Color::Rgb(166, 227, 161)), // Green
            number_style: Style::default().fg(Color::Rgb(250, 179, 135)), // Peach
            bool_style: Style::default().fg(Color::Rgb(243, 139, 168)), // Red
            null_style: Style::default().fg(Color::Rgb(245, 194, 231)), // Pink
            bracket_style: Style::default().fg(Color::Rgb(147, 153, 178)), // Overlay0
            focused_style: Style::default().bg(Color::Rgb(49, 50, 68)), // Surface0
            status_style: Style::default()
                .fg(Color::Rgb(17, 17, 27))
                .bg(Color::Rgb(203, 166, 247)), // Mauve
            indent_guide_style: Style::default().fg(Color::Rgb(88, 91, 112)), // Surface1
            error_style: Style::default().fg(Color::Rgb(243, 139, 168)), // Red
            disabled_style: Style::default().fg(Color::Rgb(88, 91, 112)), // Surface1 (dim gray)
            comment_indicator_style: Style::default().fg(Color::Rgb(143, 188, 187)), // Catppuccin Teal
            depth_key_styles: vec![
                Style::default().fg(Color::Rgb(137, 180, 250)), // Level 0 (Blue)
                Style::default().fg(Color::Rgb(203, 166, 247)), // Level 1 (Mauve)
                Style::default().fg(Color::Rgb(148, 226, 213)), // Level 2 (Teal)
                Style::default().fg(Color::Rgb(249, 226, 175)), // Level 3 (Yellow)
                Style::default().fg(Color::Rgb(245, 194, 231)), // Level 4 (Pink)
                Style::default().fg(Color::Rgb(114, 135, 253)), // Level 5 (Lavender)
            ],
            depth_bracket_styles: vec![
                Style::default().fg(Color::Rgb(147, 153, 178)), // Level 0
                Style::default().fg(Color::Rgb(186, 194, 222)), // Level 1
                Style::default().fg(Color::Rgb(166, 173, 200)), // Level 2
                Style::default().fg(Color::Rgb(147, 153, 178)), // Level 3
                Style::default().fg(Color::Rgb(186, 194, 222)), // Level 4
                Style::default().fg(Color::Rgb(166, 173, 200)), // Level 5
            ],
            scrollbar: ScrollbarStyle {
                track_symbol: "░",
                thumb_symbol: "█",
                style: Style::default().fg(Color::DarkGray),
                begin_symbol: "┐",
                end_symbol: "┘",
            },
            dropdown_scrollbar: ScrollbarStyle {
                track_symbol: "│",
                thumb_symbol: "█",
                style: Style::default().fg(Color::Rgb(147, 153, 178)),
                begin_symbol: "",
                end_symbol: "",
            },
            tooltip_scrollbar: ScrollbarStyle {
                track_symbol: "│",
                thumb_symbol: "█",
                style: Style::default().fg(Color::Rgb(147, 153, 178)),
                begin_symbol: "",
                end_symbol: "",
            },
        }
    }
}
