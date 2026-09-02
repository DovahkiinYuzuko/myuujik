use crate::i18n::I18n;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

pub struct FooterView<'a> {
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for FooterView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border_unfocused))
            .style(Style::default().bg(self.theme.bg_card));

        let inner_area = block.inner(area);
        block.render(area, buf);

        let play_label = self.i18n.t("shortcuts.play_selected");
        let vol_label = self.i18n.t("shortcuts.volume");
        let skip_label = self.i18n.t("shortcuts.next_prev_track");
        let help_label = self.i18n.t("shortcuts.help");
        let quit_label = self.i18n.t("shortcuts.quit");

        let items: [(&str, &str); 8] = [
            ("Space", "▶/❚❚"),
            ("Shift+←/→", &skip_label),
            ("Enter", &play_label),
            ("Shift+↑/↓, +/-", &vol_label),
            ("←/→", "±5s"),
            ("v", "Visual"),
            ("?", &help_label),
            ("q", &quit_label),
        ];

        let mut spans = Vec::new();
        for (i, (key, label)) in items.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("   ", Style::default().bg(self.theme.bg_card)));
            }
            spans.push(Span::styled(
                format!("[ {} ]", key),
                Style::default().fg(Color::White).bg(Color::Rgb(28, 34, 50)).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!(" {}", label),
                Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
            ));
        }

        let para = Paragraph::new(Line::from(spans)).style(Style::default().bg(self.theme.bg_card));
        para.render(inner_area, buf);
    }
}
