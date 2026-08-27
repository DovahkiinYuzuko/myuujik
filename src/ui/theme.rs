use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub primary: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub accent_playing: Color,
    pub accent_exclusive: Color,
    pub border_focus: Color,
    pub border_unfocused: Color,
    pub bg_main: Color,
    pub bg_card: Color,
    pub bg_highlight: Color,
}

impl Theme {
    pub fn default_theme() -> Self {
        Self {
            primary: Color::Rgb(0, 102, 255),          // #0066ff
            text_primary: Color::Rgb(240, 242, 248),     // High contrast off-white
            text_secondary: Color::Rgb(140, 150, 171),   // Muted slate gray
            accent_playing: Color::Rgb(82, 196, 26),     // #52c41a
            accent_exclusive: Color::Rgb(250, 173, 20),  // #faad14
            border_focus: Color::Rgb(0, 102, 255),      // Focus blue
            border_unfocused: Color::Rgb(40, 50, 72),    // Crisp dark slate border
            bg_main: Color::Rgb(12, 14, 20),             // #0c0e14 Solid terminal background
            bg_card: Color::Rgb(20, 24, 36),             // #141824 Solid card background
            bg_highlight: Color::Rgb(0, 102, 255),       // #0066ff
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_color_definitions() {
        let theme = Theme::default_theme();
        assert_eq!(theme.primary, Color::Rgb(0, 102, 255));
        assert_eq!(theme.accent_playing, Color::Rgb(82, 196, 26));
        assert_eq!(theme.accent_exclusive, Color::Rgb(250, 173, 20));
        assert_eq!(theme.bg_card, Color::Rgb(20, 24, 36));
    }
}
