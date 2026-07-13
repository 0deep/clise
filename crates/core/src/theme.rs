use ratatui::style::{Color, Style};

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
        }
    }
}
