use crate::audio::traits::AudioDeviceInfo;
use crate::fsm::FavoritesHistoryTab;
use crate::i18n::I18n;
use crate::playlist::{FavoriteTrack, HistoryItem};
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
            ("Alt+↑ / Alt+↓", self.i18n.t("shortcuts.reorder_track")),
            ("O", self.i18n.t("shortcuts.open")),
            ("/", self.i18n.t("shortcuts.search")),
            ("l", self.i18n.t("shortcuts.lyrics")),
            ("Shift+L", self.i18n.t("shortcuts.switch_language")),
            ("v", self.i18n.t("shortcuts.visualizer")),
            ("d", self.i18n.t("shortcuts.fetch_lyrics")),
            ("Shift+D", self.i18n.t("shortcuts.delete_lyrics")),
            ("a", self.i18n.t("shortcuts.queue")),
            ("r", self.i18n.t("shortcuts.repeat")),
            ("s", self.i18n.t("shortcuts.shuffle")),
            ("Shift+S", self.i18n.t("shortcuts.export_playlist")),
            ("f", self.i18n.t("help.fav_toggle")),
            ("Shift+F", self.i18n.t("help.fav_history_modal")),
            ("Shift+P", self.i18n.t("help.playlist_manager")),
            ("Delete", self.i18n.t("help.remove_from_playlist")),
            ("e", self.i18n.t("shortcuts.exclusive")),
            ("E", self.i18n.t("shortcuts.devices")),
            ("g", self.i18n.t("shortcuts.equalizer")),
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

pub struct EqualizerModal<'a> {
    pub enabled: bool,
    pub gains: &'a [f32; 10],
    pub selected_band: usize,
    pub current_preset: &'a str,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for EqualizerModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(80, 75, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("modal.equalizer"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        if inner_area.height < 12 || inner_area.width < 50 {
            return;
        }

        // 3分割レイアウト: ヘッダー(2行), スライダーエリア(Min 8行), フッター(2行)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(inner_area);

        // 1. ヘッダー: ステータス (ON/BYPASS) & プリセット名
        let status_badge = if self.enabled {
            Span::styled(format!(" [ {} ] ", self.i18n.t("modal.eq_status_on")), Style::default().fg(Color::Rgb(52, 211, 153)).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!(" [ {} ] ", self.i18n.t("modal.eq_status_bypass")), Style::default().fg(Color::Rgb(248, 113, 113)).add_modifier(Modifier::BOLD))
        };

        let header_line = Line::from(vec![
            Span::styled(format!(" {} ", self.i18n.t("modal.eq_status_label")), Style::default().fg(self.theme.text_secondary)),
            status_badge,
            Span::raw("   "),
            Span::styled(format!("{} ", self.i18n.t("modal.eq_preset_label")), Style::default().fg(self.theme.text_secondary)),
            Span::styled(format!("< {} >", self.current_preset), Style::default().fg(Color::Rgb(250, 204, 21)).add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(self.i18n.t("modal.eq_preset_hint"), Style::default().fg(self.theme.text_secondary)),
        ]);
        Paragraph::new(header_line).render(chunks[0], buf);

        // 2. スライダーエリア: 10バンド横並び
        let slider_rect = chunks[1];
        let band_labels = ["31Hz", "62Hz", "125Hz", "250Hz", "500Hz", "1kHz", "2kHz", "4kHz", "8kHz", "16kHz"];
        let band_width = (slider_rect.width as usize / 10).max(4);

        let slider_height = (slider_rect.height as usize).saturating_sub(2).max(5);
        let center_row = slider_height / 2;

        for (band_idx, &gain) in self.gains.iter().enumerate() {
            let col_x = slider_rect.x + (band_idx * band_width) as u16;
            if col_x + (band_width as u16) > slider_rect.x + slider_rect.width {
                break;
            }

            let is_selected = band_idx == self.selected_band;
            let col_style = if is_selected {
                Style::default().fg(Color::Rgb(56, 189, 248)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.text_primary)
            };

            // 上段: ゲイン数値 (例: +6.0dB)
            let gain_str = if gain > 0.0 {
                format!("+{:>4.1}", gain)
            } else {
                format!("{:>5.1}", gain)
            };
            let gain_style = if !self.enabled {
                Style::default().fg(self.theme.text_secondary)
            } else if gain > 0.0 {
                Style::default().fg(Color::Rgb(244, 114, 182))
            } else if gain < 0.0 {
                Style::default().fg(Color::Rgb(56, 189, 248))
            } else {
                Style::default().fg(self.theme.text_secondary)
            };

            // ゲイン数値の書き込み
            let label_x = col_x + (band_width.saturating_sub(gain_str.len()) / 2) as u16;
            for (ci, c) in gain_str.chars().enumerate() {
                let px = label_x + ci as u16;
                if px < slider_rect.x + slider_rect.width {
                    let cell = &mut buf[(px, slider_rect.y)];
                    cell.set_char(c);
                    cell.set_style(if is_selected { gain_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { gain_style });
                }
            }

            // 中段: 縦型スライダトラック
            // ゲイン (-12.0 .. +12.0) をスライダー行 (0 .. slider_height - 1) にマッピング
            let ratio = (gain.clamp(-12.0, 12.0) + 12.0) / 24.0; // 0.0 .. 1.0
            let knob_row = (ratio * (slider_height.saturating_sub(1) as f32)).round() as usize;
            let knob_row_from_top = slider_height.saturating_sub(1) - knob_row;

            let track_x = col_x + (band_width / 2) as u16;

            for r in 0..slider_height {
                let py = slider_rect.y + 1 + r as u16;
                if py >= slider_rect.y + slider_rect.height {
                    break;
                }
                let cell = &mut buf[(track_x, py)];

                if r == knob_row_from_top {
                    // スライダーノブ
                    cell.set_symbol("◆");
                    let knob_color = if !self.enabled {
                        self.theme.text_secondary
                    } else if is_selected {
                        Color::Rgb(250, 204, 21) // 選択中はイエローハイライト
                    } else {
                        Color::White
                    };
                    cell.set_fg(knob_color);
                } else if r == center_row {
                    // 0dB センターライン
                    cell.set_symbol("┼");
                    cell.set_fg(self.theme.border_unfocused);
                } else {
                    // レール
                    cell.set_symbol("│");
                    cell.set_fg(if is_selected { Color::Rgb(56, 189, 248) } else { self.theme.border_unfocused });
                }
            }

            // 下段: 周波数ラベル (例: 1kHz)
            let freq_label = band_labels[band_idx];
            let freq_x = col_x + (band_width.saturating_sub(freq_label.len()) / 2) as u16;
            let freq_y = slider_rect.y + 1 + slider_height as u16;

            for (ci, c) in freq_label.chars().enumerate() {
                let px = freq_x + ci as u16;
                if px < slider_rect.x + slider_rect.width && freq_y < slider_rect.y + slider_rect.height {
                    let cell = &mut buf[(px, freq_y)];
                    cell.set_char(c);
                    cell.set_style(col_style);
                }
            }
        }

        // 3. フッター: キー操作ガイド
        let footer_line = Line::from(vec![
            Span::styled(" [←/→] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("modal.eq_guide_band")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[↑/↓] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("modal.eq_guide_gain")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Shift+↑/↓] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("modal.eq_guide_gain_fast")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Space] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("modal.eq_guide_bypass")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Esc/g] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(self.i18n.t("modal.eq_guide_close"), Style::default().fg(self.theme.text_primary)),
        ]);
        Paragraph::new(footer_line).render(chunks[2], buf);
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

pub struct FavoritesHistoryModal<'a> {
    pub tab: FavoritesHistoryTab,
    pub selected_idx: usize,
    pub favorites: &'a [FavoriteTrack],
    pub history: &'a [HistoryItem],
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for FavoritesHistoryModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(75, 75, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("library.modal_title"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        if inner_area.height < 4 {
            return;
        }

        // 上部: タブバー (1行)
        // 中央: アイテムリスト (可変)
        // 下部: 操作ガイド (1行)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // タブ
                Constraint::Min(1),    // リスト
                Constraint::Length(1), // フッターガイド
            ])
            .split(inner_area);

        // 1. タブバー
        let fav_tab_title = self.i18n.t_args("library.tab_favorites", &[("count", &self.favorites.len().to_string())]);
        let hist_tab_title = self.i18n.t_args("library.tab_history", &[("count", &self.history.len().to_string())]);

        let (fav_style, hist_style) = match self.tab {
            FavoritesHistoryTab::Favorites => (
                Style::default().bg(self.theme.primary).fg(Color::White).add_modifier(Modifier::BOLD),
                Style::default().bg(self.theme.bg_card).fg(self.theme.text_secondary),
            ),
            FavoritesHistoryTab::History => (
                Style::default().bg(self.theme.bg_card).fg(self.theme.text_secondary),
                Style::default().bg(self.theme.primary).fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        };

        let tabs_line = Line::from(vec![
            Span::styled(format!("  {}  ", fav_tab_title), fav_style),
            Span::styled("   ", Style::default().bg(self.theme.bg_card)),
            Span::styled(format!("  {}  ", hist_tab_title), hist_style),
        ]);
        Paragraph::new(tabs_line).style(Style::default().bg(self.theme.bg_card)).render(chunks[0], buf);

        // 2. リスト本体
        match self.tab {
            FavoritesHistoryTab::Favorites => {
                if self.favorites.is_empty() {
                    let empty_text = self.i18n.t("library.favorites_empty");
                    let p = Paragraph::new(Line::from(Span::styled(
                        format!("\n  {}", empty_text),
                        Style::default().fg(self.theme.text_secondary),
                    )))
                    .style(Style::default().bg(self.theme.bg_card));
                    p.render(chunks[1], buf);
                } else {
                    let items: Vec<ListItem> = self
                        .favorites
                        .iter()
                        .enumerate()
                        .map(|(idx, fav)| {
                            let is_sel = idx == self.selected_idx;
                            let prefix = if is_sel { " ▶ " } else { "   " };
                            let (y, m, d, h, min, _) = crate::logger::epoch_secs_to_utc_ymd(fav.added_at);
                            let time_str = format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, h, min);

                            let line = Line::from(vec![
                                Span::styled(prefix, if is_sel { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(self.theme.text_secondary) }),
                                Span::styled("★ ", Style::default().fg(Color::Rgb(251, 191, 36)).add_modifier(Modifier::BOLD)),
                                Span::styled(
                                    format!("{:<40}", fav.display_name),
                                    if is_sel { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(self.theme.text_primary) },
                                ),
                                Span::styled(
                                    format!("  [{}]", time_str),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                            ]);

                            let item_style = if is_sel {
                                Style::default().bg(self.theme.primary).fg(Color::White)
                            } else {
                                Style::default().bg(self.theme.bg_card)
                            };
                            ListItem::new(line).style(item_style)
                        })
                        .collect();

                    let list = List::new(items).style(Style::default().bg(self.theme.bg_card));
                    list.render(chunks[1], buf);
                }
            }
            FavoritesHistoryTab::History => {
                if self.history.is_empty() {
                    let empty_text = self.i18n.t("library.history_empty");
                    let p = Paragraph::new(Line::from(Span::styled(
                        format!("\n  {}", empty_text),
                        Style::default().fg(self.theme.text_secondary),
                    )))
                    .style(Style::default().bg(self.theme.bg_card));
                    p.render(chunks[1], buf);
                } else {
                    let items: Vec<ListItem> = self
                        .history
                        .iter()
                        .enumerate()
                        .map(|(idx, item)| {
                            let is_sel = idx == self.selected_idx;
                            let prefix = if is_sel { " ▶ " } else { "   " };
                            let (y, m, d, h, min, _) = crate::logger::epoch_secs_to_utc_ymd(item.played_at);
                            let time_str = format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, h, min);

                            let line = Line::from(vec![
                                Span::styled(prefix, if is_sel { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(self.theme.text_secondary) }),
                                Span::styled("⏱ ", Style::default().fg(Color::Rgb(56, 189, 248))),
                                Span::styled(
                                    format!("{:<40}", item.display_name),
                                    if is_sel { Style::default().fg(Color::White).add_modifier(Modifier::BOLD) } else { Style::default().fg(self.theme.text_primary) },
                                ),
                                Span::styled(
                                    format!("  [{}] ({}x)", time_str, item.play_count),
                                    Style::default().fg(self.theme.text_secondary),
                                ),
                            ]);

                            let item_style = if is_sel {
                                Style::default().bg(self.theme.primary).fg(Color::White)
                            } else {
                                Style::default().bg(self.theme.bg_card)
                            };
                            ListItem::new(line).style(item_style)
                        })
                        .collect();

                    let list = List::new(items).style(Style::default().bg(self.theme.bg_card));
                    list.render(chunks[1], buf);
                }
            }
        }

        // 3. フッターガイド
        let footer_line = Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("library.action_play")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Tab] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("library.action_tab")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[d] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("library.action_delete")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Esc] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(self.i18n.t("library.action_close"), Style::default().fg(self.theme.text_primary)),
        ]);
        Paragraph::new(footer_line).render(chunks[2], buf);
    }
}

pub struct PlaylistManagerModal<'a> {
    pub playlists: &'a [crate::playlist::CustomPlaylistInfo],
    pub selected_index: usize,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for PlaylistManagerModal<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let modal_area = centered_rect(70, 60, area);
        Clear.render(modal_area, buf);

        let title = format!(" [ {} ] ", self.i18n.t("custom_playlist.modal_title"));
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.primary))
            .style(Style::default().bg(self.theme.bg_card))
            .title(Span::styled(
                title,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(modal_area);
        block.render(modal_area, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),    // リスト本体
                Constraint::Length(2), // 操作ガイド
            ])
            .split(inner_area);

        if self.playlists.is_empty() {
            let empty_text = format!("\n  {}", self.i18n.t("custom_playlist.empty"));
            let empty_para = Paragraph::new(empty_text)
                .style(Style::default().fg(self.theme.text_secondary).bg(self.theme.bg_card));
            empty_para.render(chunks[0], buf);
        } else {
            let items: Vec<ListItem> = self
                .playlists
                .iter()
                .enumerate()
                .map(|(idx, pl)| {
                    let is_selected = idx == self.selected_index;
                    let prefix = if is_selected { " ▶ " } else { "   " };
                    let count_str = self.i18n.t_args("custom_playlist.tracks_count", &[("count", &pl.track_count.to_string())]);

                    let line = Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default().fg(if is_selected { self.theme.primary } else { self.theme.text_secondary }),
                        ),
                        Span::styled(
                            format!("{:<30}", pl.name),
                            Style::default().fg(if is_selected { Color::White } else { self.theme.text_primary }).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
                        ),
                        Span::styled(
                            format!("  [{}]", count_str),
                            Style::default().fg(self.theme.text_secondary),
                        ),
                    ]);

                    let item_style = if is_selected {
                        Style::default().bg(self.theme.primary).fg(Color::White)
                    } else {
                        Style::default().bg(self.theme.bg_card)
                    };
                    ListItem::new(line).style(item_style)
                })
                .collect();

            let list = List::new(items).style(Style::default().bg(self.theme.bg_card));
            list.render(chunks[0], buf);
        }

        let footer_line = Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("custom_playlist.action_load")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[s] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("custom_playlist.action_save")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[a] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("custom_playlist.action_append")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[d] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}  ", self.i18n.t("custom_playlist.action_delete")), Style::default().fg(self.theme.text_primary)),
            Span::styled("[Esc] ", Style::default().fg(self.theme.primary).add_modifier(Modifier::BOLD)),
            Span::styled(self.i18n.t("custom_playlist.action_close"), Style::default().fg(self.theme.text_primary)),
        ]);
        Paragraph::new(footer_line).render(chunks[1], buf);
    }
}

