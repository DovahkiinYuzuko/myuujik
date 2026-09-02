use crate::audio::traits::AudioDeviceInfo;
use crate::i18n::I18n;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget};

pub struct DeviceSelectModal<'a> {
    pub devices: &'a [AudioDeviceInfo],
    pub selected_idx: usize,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for DeviceSelectModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(60, 50, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("modal.device_select"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        let default_badge_str = self.i18n.t("modal.default_badge");
        let items: Vec<ListItem> = self
            .devices
            .iter()
            .enumerate()
            .map(|(idx, dev)| {
                let is_sel = idx == self.selected_idx;
                let prefix = if is_sel { " ▶ " } else { "   " };
                let def_badge = if dev.is_default { default_badge_str.as_str() } else { "" };
                let text = format!("{}{}{}", prefix, dev.name, def_badge);

                let style = if is_sel {
                    Style::default().bg(self.theme.primary).fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().bg(self.theme.bg_card).fg(self.theme.text_primary)
                };
                ListItem::new(text).style(style)
            })
            .collect();

        let list = List::new(items).style(Style::default().bg(self.theme.bg_card));
        list.render(inner_area, buf);
    }
}

pub struct HelpModal<'a> {
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for HelpModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(70, 70, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("modal.help"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        let shortcuts: Vec<(&str, String)> = vec![
            ("Space", self.i18n.t("shortcuts.play_pause")),
            ("Enter", self.i18n.t("shortcuts.play_selected")),
            ("Backspace", self.i18n.t("shortcuts.parent_dir")),
            ("↑ / ↓", self.i18n.t("shortcuts.select_track")),
            ("← / →", self.i18n.t("shortcuts.seek")),
            ("Shift+← / Shift+→", self.i18n.t("shortcuts.next_prev_track")),
            ("Shift+↑/↓, +/-", self.i18n.t("shortcuts.volume")),
            ("O", self.i18n.t("shortcuts.open")),
            ("/", self.i18n.t("shortcuts.search")),
            ("l", self.i18n.t("shortcuts.lyrics")),
            ("v", self.i18n.t("shortcuts.visualizer")),
            ("d", self.i18n.t("shortcuts.fetch_lyrics")),
            ("Shift+D", self.i18n.t("shortcuts.delete_lyrics")),
            ("a", self.i18n.t("shortcuts.queue")),
            ("r", self.i18n.t("shortcuts.repeat")),
            ("s", self.i18n.t("shortcuts.shuffle")),
            ("e", self.i18n.t("shortcuts.exclusive")),
            ("E", self.i18n.t("shortcuts.devices")),
            ("Tab / Shift+Tab", self.i18n.t("shortcuts.pane_switch")),
            ("q / Esc", self.i18n.t("shortcuts.quit")),
        ];

        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("--- {} ---", self.i18n.t("modal.key_reference")),
            Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
        )));
        lines.push(Line::from(""));

        for (k, desc) in shortcuts.iter() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:20}", k), Style::default().fg(self.theme.primary).bg(self.theme.bg_card).add_modifier(Modifier::BOLD)),
                Span::styled(format!(": {}", desc), Style::default().fg(self.theme.text_primary).bg(self.theme.bg_card)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  [ {} ]", self.i18n.t("modal.press_esc_q")),
            Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
        )));

        let para = Paragraph::new(lines).style(Style::default().bg(self.theme.bg_card));
        para.render(inner_area, buf);
    }
}

pub struct ErrorModal<'a> {
    pub message: &'a str,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for ErrorModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(50, 30, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("modal.error"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(224, 32, 32)))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(title, Style::default().fg(Color::Rgb(224, 32, 32)).add_modifier(Modifier::BOLD)));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", self.message),
                Style::default().fg(Color::White).bg(self.theme.bg_card),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("  [ {} ]", self.i18n.t("modal.press_dismiss")),
                Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
            )),
        ];

        let para = Paragraph::new(lines).style(Style::default().bg(self.theme.bg_card));
        para.render(inner_area, buf);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
