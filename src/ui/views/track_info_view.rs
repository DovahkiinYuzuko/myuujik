use crate::audio::decoder::TrackMetadata;
use crate::i18n::I18n;
use crate::ui::image_view::CoverArtWidget;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

pub struct TrackInfoView<'a> {
    pub metadata: Option<&'a TrackMetadata>,
    pub output_mode: &'a str,
    pub is_exclusive: bool,
    pub is_fallback: bool,
    pub is_focused: bool,
    pub cover_widget: &'a mut CoverArtWidget,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> TrackInfoView<'a> {
    pub fn render_view(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_focused {
            self.theme.border_focus
        } else {
            self.theme.border_unfocused
        };

        let title = format!(" [ {} ] ", self.i18n.t("track_info.header"));
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

        // 左右分割：左側カバーアート（幅26）、右側テキスト情報
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(26), // カバーアート領域
                Constraint::Min(20),    // メタデータ領域
            ])
            .split(inner_area);

        // カバーアート描画（画像プロトコルまたはASCIIアートフォールバック）
        if self.cover_widget.has_image() {
            self.cover_widget.render(chunks[0], buf);
        } else {
            let art_box = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.border_unfocused))
                .style(Style::default().bg(Color::Rgb(15, 18, 26)));
            let art_inner = art_box.inner(chunks[0]);
            art_box.render(chunks[0], buf);

            let placeholder = vec![
                Line::from(""),
                Line::from(Span::styled("   ┌──────────┐", Style::default().fg(self.theme.border_unfocused))),
                Line::from(Span::styled("   │    ■     │", Style::default().fg(self.theme.text_secondary).add_modifier(Modifier::BOLD))),
                Line::from(Span::styled(format!("   │ {} │", self.i18n.t("track_info.no_album_art_line1")), Style::default().fg(self.theme.text_secondary))),
                Line::from(Span::styled(format!("   │   {}    │", self.i18n.t("track_info.no_album_art_line2")), Style::default().fg(self.theme.text_secondary))),
                Line::from(Span::styled("   └──────────┘", Style::default().fg(self.theme.border_unfocused))),
            ];
            let para = Paragraph::new(placeholder).style(Style::default().bg(Color::Rgb(15, 18, 26)));
            para.render(art_inner, buf);
        }

        // テキスト情報
        let mut lines = Vec::new();
        if let Some(meta) = self.metadata {
            let track_title = meta.title.clone().unwrap_or_else(|| {
                meta.file_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| self.i18n.t("track_info.unknown_track"))
            });
            let artist = meta.artist.clone().unwrap_or_else(|| self.i18n.t("track_info.unknown_artist"));
            let album = meta.album.clone().unwrap_or_else(|| self.i18n.t("track_info.unknown_album"));

            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", self.i18n.t("track_info.title")), Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card)),
                Span::styled(track_title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD).bg(self.theme.bg_card)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", self.i18n.t("track_info.artist")), Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card)),
                Span::styled(artist, Style::default().fg(self.theme.text_primary).bg(self.theme.bg_card)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", self.i18n.t("track_info.album")), Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card)),
                Span::styled(album, Style::default().fg(self.theme.text_primary).bg(self.theme.bg_card)),
            ]));

            let format_str = format!(
                "{} | {:.1} kHz | {}-bit | {} ch",
                meta.codec_name,
                meta.sample_rate as f64 / 1000.0,
                meta.bits_per_sample.unwrap_or(16),
                meta.channels
            );

            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", self.i18n.t("track_info.format")), Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card)),
                Span::styled(format_str, Style::default().fg(self.theme.primary).bg(self.theme.bg_card)),
            ]));

            let mode_badge = if self.is_exclusive {
                Span::styled(format!(" [ {} ] ", self.i18n.t("track_info.badge_exclusive")), Style::default().fg(self.theme.accent_exclusive).bg(Color::Rgb(35, 25, 10)).add_modifier(Modifier::BOLD))
            } else if self.is_fallback {
                Span::styled(format!(" [ {} ] ", self.i18n.t("track_info.badge_shared_fallback")), Style::default().fg(Color::Yellow).bg(Color::Rgb(35, 30, 10)).add_modifier(Modifier::BOLD))
            } else {
                Span::styled(format!(" [ {} ] ", self.i18n.t("track_info.badge_shared")), Style::default().fg(self.theme.primary).bg(Color::Rgb(15, 25, 45)))
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", self.i18n.t("track_info.output_mode")), Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card)),
                mode_badge,
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                self.i18n.t("track_info.no_track_loaded"),
                Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card),
            )));
        }

        let para = Paragraph::new(lines).style(Style::default().bg(self.theme.bg_card));
        para.render(chunks[1], buf);
    }
}
