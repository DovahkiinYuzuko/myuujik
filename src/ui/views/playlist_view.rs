use crate::i18n::I18n;
use crate::playlist::item::PlaylistEntry;
use crate::playlist::manager::PlaylistManager;
use crate::ui::theme::Theme;
use crate::ui::ticker::{str_width, take_cells, MarqueeTicker};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget};

pub struct PlaylistView<'a> {
    pub playlist: &'a PlaylistManager,
    pub is_focused: bool,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
    pub elapsed_ms: u128,
}

impl<'a> Widget for PlaylistView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_focused {
            self.theme.border_focus
        } else {
            self.theme.border_unfocused
        };

        let title = if let Some(q) = self.playlist.filter_query() {
            format!(
                " [ {} ({} \"{}\": {}/{}) ] ",
                self.i18n.t("playlist.header"),
                self.i18n.t("search.prompt"),
                q,
                self.playlist.len(),
                self.playlist.all_tracks().len()
            )
        } else {
            format!(" [ {} ({}) ] ", self.i18n.t("playlist.header"), self.playlist.len())
        };
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

        if inner_area.height < 2 {
            return;
        }

        // 上部1行にパンくずリストバー、残りにアイテム一覧を配置
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // パンくずリスト
                Constraint::Min(1),    // リスト本体
            ])
            .split(inner_area);

        // 1. パンくずリスト
        let breadcrumb_text = if self.playlist.is_filtered() {
            format!("> [Search: \"{}\"]", self.playlist.filter_query().unwrap_or(""))
        } else {
            self.playlist.breadcrumb()
        };
        let breadcrumb_para = Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                breadcrumb_text,
                Style::default()
                    .fg(Color::Rgb(251, 191, 36))
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(Style::default().bg(self.theme.bg_card));
        breadcrumb_para.render(chunks[0], buf);

        if self.playlist.is_empty() {
            let msg = if self.playlist.is_filtered() {
                self.i18n.t("search.no_results")
            } else {
                self.i18n.t("playlist.empty")
            };
            let empty_msg = Line::from(Span::styled(
                format!("  {}", msg),
                Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
            ));
            let list = List::new(vec![ListItem::new(empty_msg)]).style(Style::default().bg(self.theme.bg_card));
            list.render(chunks[1], buf);
            return;
        }

        let current_track_path = self.playlist.current_track_path();
        let cursor_idx = self.playlist.cursor();
        let ticker = MarqueeTicker::default();

        let list_items: Vec<ListItem> = self
            .playlist
            .entries()
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_cursor = idx == cursor_idx;
                let is_playing = match entry {
                    PlaylistEntry::AudioFile(item) => {
                        current_track_path.map(|p| p == &item.path).unwrap_or(false)
                    }
                    _ => false,
                };

                let prefix = if is_cursor && is_playing {
                    " ▶▶ "
                } else if is_playing {
                    "  ▶ "
                } else if is_cursor {
                    "  > "
                } else {
                    "    "
                };

                let (badge_str, badge_color): (String, Color) = match entry {
                    PlaylistEntry::ParentDir => ("[UP] ".to_string(), Color::Rgb(192, 132, 252)),
                    PlaylistEntry::Directory { .. } => ("[DIR] ".to_string(), Color::Rgb(251, 191, 36)),
                    PlaylistEntry::AudioFile(item) => {
                        let ext = item
                            .path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("AUD")
                            .to_uppercase();
                        let col = match ext.as_str() {
                            "FLAC" | "ALAC" => Color::Rgb(56, 189, 248),
                            "WAV" | "WAVE" => Color::Rgb(52, 211, 153),
                            "MP3" => Color::Rgb(244, 114, 182),
                            _ => Color::Rgb(148, 163, 184),
                        };
                        (format!("[{}] ", ext), col)
                    }
                };

                let raw_name = entry.display_name();

                // プレフィックス＋バッジのセル幅
                let fixed_width = str_width(prefix) + str_width(&badge_str);
                let available_name_width = (chunks[1].width as usize).saturating_sub(fixed_width + 1);

                // カーソル行なら電光掲示板マーキースクロール、それ以外は枠幅カット
                let display_name = if is_cursor {
                    ticker.render(raw_name, available_name_width, self.elapsed_ms)
                } else {
                    take_cells(raw_name, available_name_width)
                };

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
                        badge_str,
                        Style::default().fg(badge_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        display_name,
                        if is_cursor {
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                        } else if is_playing {
                            Style::default().fg(self.theme.accent_playing).add_modifier(Modifier::BOLD)
                        } else if matches!(entry, PlaylistEntry::Directory { .. }) {
                            Style::default().fg(Color::Rgb(251, 191, 36))
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
        list.render(chunks[1], buf);
    }
}
