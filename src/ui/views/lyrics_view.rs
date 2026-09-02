use crate::audio::lyrics::Lyrics;
use crate::i18n::I18n;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

/// 同期歌詞の自動スクロール＆現在歌唱行ハイライト描画ウィジェット
pub struct LyricsView<'a> {
    pub lyrics: Option<&'a Lyrics>,
    pub elapsed_ms: u64,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for LyricsView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let Some(lyrics) = self.lyrics else {
            let empty_text = self.i18n.t("lyrics.not_found");
            let p = Paragraph::new(Line::from(Span::styled(
                empty_text,
                Style::default().fg(self.theme.text_secondary),
            )))
            .alignment(Alignment::Center);
            let y = area.y + area.height / 2;
            p.render(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            return;
        };

        if lyrics.lines.is_empty() {
            let empty_text = self.i18n.t("lyrics.not_found");
            let p = Paragraph::new(Line::from(Span::styled(
                empty_text,
                Style::default().fg(self.theme.text_secondary),
            )))
            .alignment(Alignment::Center);
            let y = area.y + area.height / 2;
            p.render(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
            return;
        }

        // 現在歌唱中の行インデックス
        let current_idx = lyrics.current_line_index(self.elapsed_ms).unwrap_or(0);
        let center_y = area.height / 2;

        for row in 0..area.height {
            let target_line_idx = (current_idx as i64) + (row as i64) - (center_y as i64);
            if target_line_idx < 0 || target_line_idx >= lyrics.lines.len() as i64 {
                continue;
            }

            let idx = target_line_idx as usize;
            let line = &lyrics.lines[idx];
            let is_current = idx == current_idx;

            let (style, prefix) = if is_current {
                (
                    Style::default()
                        .fg(Color::Rgb(56, 189, 248)) // 明るいシアン（ハイライト）
                        .add_modifier(Modifier::BOLD),
                    "▶ ",
                )
            } else {
                let dist = (idx as i64 - current_idx as i64).abs();
                let fg_color = if dist == 1 {
                    Color::Rgb(200, 210, 230) // 直近行は少し明るく
                } else {
                    self.theme.text_secondary // 離れた行は落ち着いた色
                };
                (Style::default().fg(fg_color), "  ")
            };

            let content = format!("  {}{}", prefix, line.text);
            let p = Paragraph::new(Line::from(Span::styled(content, style)))
                .alignment(Alignment::Left);

            p.render(
                Rect {
                    x: area.x,
                    y: area.y + row,
                    width: area.width,
                    height: 1,
                },
                buf,
            );
        }
    }
}
