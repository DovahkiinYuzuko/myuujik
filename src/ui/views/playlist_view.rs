use crate::i18n::I18n;
use crate::playlist::manager::PlaylistManager;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};

pub struct PlaylistView<'a> {
    pub playlist: &'a PlaylistManager,
    pub is_focused: bool,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for PlaylistView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_focused {
            self.theme.border_focus
        } else {
            self.theme.border_unfocused
        };

        let title = format!(" [ {} ({}) ] ", self.i18n.t("playlist.header"), self.playlist.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(if self.is_focused { Color::White } else { self.theme.text_secondary })
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        block.render(area, buf);

        if self.playlist.is_empty() {
            let empty_msg = Line::from(Span::styled(
                format!("  {}", self.i18n.t("playlist.empty")),
                Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
            ));
            let list = List::new(vec![ListItem::new(empty_msg)]).style(Style::default().bg(self.theme.bg_card));
            list.render(inner_area, buf);
            return;
        }

        let current_playing_idx = self.playlist.current_playing_index();
        let cursor_idx = self.playlist.cursor();

        let list_items: Vec<ListItem> = self
            .playlist
            .items()
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_cursor = idx == cursor_idx;
                let is_playing = Some(idx) == current_playing_idx;

                let prefix = if is_cursor && is_playing {
                    " ▶▶ "
                } else if is_playing {
                    "  ▶ "
                } else if is_cursor {
                    "  > "
                } else {
                    "    "
                };

                let num_str = format!("{:02}. ", idx + 1);
                let name_str = item.display_name.clone();

                let line = Line::from(vec![
                    Span::styled(
                        prefix,
                        if is_cursor {
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                        } else if is_playing {
                            Style::default().fg(self.theme.accent_playing).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_secondary)
                        },
                    ),
                    Span::styled(
                        num_str,
                        if is_cursor {
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_secondary)
                        },
                    ),
                    Span::styled(
                        name_str,
                        if is_cursor {
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                        } else if is_playing {
                            Style::default().fg(self.theme.accent_playing).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(self.theme.text_primary)
                        },
                    ),
                ]);

                let item_style = if is_cursor {
                    Style::default().bg(self.theme.primary).fg(Color::White)
                } else {
                    Style::default().bg(self.theme.bg_card)
                };

                ListItem::new(line).style(item_style)
            })
            .collect();

        let list = List::new(list_items).style(Style::default().bg(self.theme.bg_card));
        list.render(inner_area, buf);
    }
}
