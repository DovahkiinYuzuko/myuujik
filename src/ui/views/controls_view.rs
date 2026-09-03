use crate::audio::lyrics::Lyrics;
use crate::audio::visualizer::VisualizerMode;
use crate::fsm::playback_fsm::PlaybackState;
use crate::i18n::I18n;
use crate::playlist::manager::RepeatMode;
use crate::ui::theme::Theme;
use crate::ui::views::lyrics_view::LyricsView;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Widget};

pub struct ControlsView<'a> {
    pub playback_state: &'a PlaybackState,
    pub current_position_secs: f64,
    pub total_duration_secs: f64,
    pub volume: f32,
    pub repeat_mode: RepeatMode,
    pub is_shuffle: bool,
    pub is_focused: bool,
    pub visualizer_mode: VisualizerMode,
    pub waveform_points: &'a [f32],
    pub spectrum_bands: (&'a [f32], &'a [f32]),
    pub lyrics: Option<&'a Lyrics>,
    pub show_lyrics: bool,
    pub is_fetching_lyrics: bool,
    pub lyrics_toast: Option<&'a (String, std::time::Instant, bool)>,
    pub next_queue_track: Option<&'a str>,
    pub i18n: &'a I18n,
    pub theme: &'a Theme,
}

impl<'a> Widget for ControlsView<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_color = if self.is_focused {
            self.theme.border_focus
        } else {
            self.theme.border_unfocused
        };

        let status_badge = match self.playback_state {
            PlaybackState::Playing => format!("● {}", self.i18n.t("controls.status_playing")),
            PlaybackState::Paused => format!("❚❚ {}", self.i18n.t("controls.status_paused")),
            PlaybackState::Stopped => format!("■ {}", self.i18n.t("controls.status_stopped")),
            PlaybackState::Buffering { .. } => format!("⟳ {}", self.i18n.t("controls.status_buffering")),
            PlaybackState::Seeking { .. } => format!("⟲ {}", self.i18n.t("controls.status_seeking")),
            PlaybackState::TrackChanging { .. } => format!("⟳ {}", self.i18n.t("controls.status_changing")),
            PlaybackState::Error { .. } => format!("▲ {}", self.i18n.t("controls.status_error")),
        };

        let title = format!(" [ {} ] ", self.i18n.t("controls.header"));
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

        // 垂直分割（シークバー、ステータス、波形スパークライン）
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // プログレスバー
                Constraint::Length(1), // ステータス行
                Constraint::Min(2),    // リアルタイム波形スパークライン
            ])
            .split(inner_area);

        // 1. プログレスバー
        let ratio = if self.total_duration_secs > 0.0 {
            (self.current_position_secs / self.total_duration_secs).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let cur_min = (self.current_position_secs / 60.0) as u32;
        let cur_sec = (self.current_position_secs % 60.0) as u32;
        let tot_min = (self.total_duration_secs / 60.0) as u32;
        let tot_sec = (self.total_duration_secs % 60.0) as u32;

        let time_str = format!("{:02}:{:02} / {:02}:{:02}", cur_min, cur_sec, tot_min, tot_sec);

        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(self.theme.primary).bg(Color::Rgb(25, 30, 45)))
            .ratio(ratio)
            .label(Span::styled(time_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)));
        gauge.render(chunks[0], buf);

        // インタラクティブシークバー: 再生ヘッドつまみ (●) の描画
        if chunks[0].width > 0 && chunks[0].height > 0 {
            let head_x = chunks[0].x + (ratio * (chunks[0].width.saturating_sub(1) as f64)).round() as u16;
            if head_x < chunks[0].x + chunks[0].width {
                if let Some(cell) = buf.cell_mut((head_x, chunks[0].y)) {
                    cell.set_symbol("●");
                    cell.set_style(Style::default().fg(Color::Rgb(255, 255, 255)).bg(self.theme.primary).add_modifier(Modifier::BOLD));
                }
            }
        }

        // 2. ステータス行
        let repeat_str = match self.repeat_mode {
            RepeatMode::Off => self.i18n.t("controls.loop_off"),
            RepeatMode::All => self.i18n.t("controls.loop_all"),
            RepeatMode::Single => self.i18n.t("controls.loop_single"),
        };

        let shuffle_str = if self.is_shuffle {
            self.i18n.t("controls.shuf_on")
        } else {
            self.i18n.t("controls.shuf_off")
        };

        let vol_percent = (self.volume * 100.0).round() as u32;
        let vol_pct_str = vol_percent.to_string();
        let vol_str = self.i18n.t_args("controls.vol_label", &[("val", &vol_pct_str)]);

        let mode_label = if self.show_lyrics {
            self.i18n.t("lyrics.title")
        } else {
            self.visualizer_mode.display_name().to_string()
        };

        let mut status_spans = vec![
            Span::styled(
                format!(" {} ", status_badge),
                Style::default()
                    .fg(match self.playback_state {
                        PlaybackState::Playing => self.theme.accent_playing,
                        PlaybackState::Paused => self.theme.accent_exclusive,
                        _ => self.theme.text_secondary,
                    })
                    .add_modifier(Modifier::BOLD)
                    .bg(self.theme.bg_card),
            ),
            Span::raw(" "),
            Span::styled(
                " [ |◀ ] ",
                Style::default().fg(self.theme.text_primary).bg(Color::Rgb(28, 34, 50)).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                " [ ▶| ] ",
                Style::default().fg(self.theme.text_primary).bg(Color::Rgb(28, 34, 50)).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" [ {} ] ", repeat_str),
                Style::default().fg(self.theme.text_primary).bg(Color::Rgb(28, 34, 50)),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" [ {} ] ", shuffle_str),
                Style::default().fg(self.theme.text_primary).bg(Color::Rgb(28, 34, 50)),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" [ {} ] ", vol_str),
                Style::default().fg(self.theme.primary).bg(Color::Rgb(15, 25, 45)).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!(" [ {} ] ", mode_label),
                Style::default().fg(Color::Rgb(56, 189, 248)).bg(Color::Rgb(20, 28, 45)).add_modifier(Modifier::BOLD),
            ),
        ];

        if let Some((ref msg, _, is_err)) = self.lyrics_toast {
            let color = if *is_err {
                Color::Rgb(239, 68, 68)
            } else {
                Color::Rgb(56, 189, 248)
            };
            status_spans.push(Span::raw("  "));
            status_spans.push(Span::styled(
                format!(" ✦ {} ", msg),
                Style::default().fg(color).bg(Color::Rgb(25, 30, 45)).add_modifier(Modifier::BOLD),
            ));
        } else if let Some(next_title) = self.next_queue_track {
            let label = self.i18n.t_args("queue.next_label", &[("track", next_title)]);
            status_spans.push(Span::raw("  "));
            status_spans.push(Span::styled(
                format!(" ⮞ {} ", label),
                Style::default().fg(Color::Rgb(245, 158, 11)).bg(Color::Rgb(35, 30, 20)).add_modifier(Modifier::BOLD),
            ));
        }

        let status_para = Paragraph::new(Line::from(status_spans)).style(Style::default().bg(self.theme.bg_card));
        status_para.render(chunks[1], buf);

        // 3. ビジュアライザまたは同期歌詞描画
        if chunks[2].height > 0 && chunks[2].width > 0 {
            if self.show_lyrics {
                let elapsed_ms = (self.current_position_secs * 1000.0).max(0.0) as u64;
                let fetch_message = self.lyrics_toast.as_ref().map(|(msg, _, _)| msg.as_str());
                let is_fetch_error = self.lyrics_toast.as_ref().map(|(_, _, is_err)| *is_err).unwrap_or(false);
                let lyrics_view = LyricsView {
                    lyrics: self.lyrics,
                    elapsed_ms,
                    is_fetching: self.is_fetching_lyrics,
                    fetch_message,
                    is_fetch_error,
                    i18n: self.i18n,
                    theme: self.theme,
                };
                lyrics_view.render(chunks[2], buf);
            } else {
                match self.visualizer_mode {
                VisualizerMode::Type3 => {
                    // METER: 等幅スペーシング（バー1列：スペース1列）による複数行バーグラフ (底辺接地)
                    let blocks = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                    let width = chunks[2].width as usize;
                    let height = chunks[2].height as usize;

                    let mut lines = Vec::with_capacity(height);
                    for row in 0..height {
                        let bottom_up = height - 1 - row;
                        let mut line_chars = String::with_capacity(width);

                        for col in 0..width {
                            // 等幅スペーシング: 偶数列がバー、奇数列がスペース（間にバー1個分の隙間）
                            if col % 2 == 1 {
                                line_chars.push(' ');
                                continue;
                            }

                            let bar_idx = col / 2;
                            let val = if bar_idx < self.waveform_points.len() {
                                self.waveform_points[bar_idx].clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let total_bar_height = val * (height as f32);

                            let ch = if total_bar_height >= (bottom_up + 1) as f32 {
                                '█'
                            } else if total_bar_height <= bottom_up as f32 {
                                ' '
                            } else {
                                let frac = total_bar_height - bottom_up as f32;
                                let idx = ((frac * 8.0).round() as usize).clamp(1, 8);
                                blocks[idx]
                            };
                            line_chars.push(ch);
                        }

                        lines.push(Line::from(Span::styled(
                            line_chars,
                            Style::default()
                                .fg(Color::Rgb(56, 189, 248))
                                .bg(self.theme.bg_card)
                                .add_modifier(Modifier::BOLD),
                        )));
                    }

                    Paragraph::new(lines).render(chunks[2], buf);
                }
                VisualizerMode::Type4 => {
                    // WAVE: 生のPCM連続波形 (スパークライン)
                    let spark_data: Vec<u64> = self
                        .waveform_points
                        .iter()
                        .map(|&p| (p * 100.0).clamp(0.0, 100.0) as u64)
                        .collect();
                    let sparkline = Sparkline::default()
                        .style(Style::default().fg(Color::Rgb(56, 189, 248)).bg(self.theme.bg_card))
                        .data(&spark_data)
                        .max(100);
                    sparkline.render(chunks[2], buf);
                }
                VisualizerMode::Spectrum => {
                    // SPECTRUM: 本格対数周波数スペアナ（ブロック文字 ▂▃▄▅▆▇█ ＋ ピークドット ▔）
                    let rect = chunks[2];
                    let width = rect.width as usize;
                    let height = rect.height as usize;
                    if width > 0 && height > 0 {
                        let (bars, peaks) = self.spectrum_bands;
                        let blocks = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                        let num_bars = (width + 1) / 2;

                        for bar_idx in 0..num_bars {
                            let col = bar_idx * 2;
                            if col >= width {
                                break;
                            }

                            // バンドデータのマッピング（64バンドからnum_barsへのリサンプリング）
                            let (val, peak) = if !bars.is_empty() {
                                let ratio = bar_idx as f32 / num_bars.max(1) as f32;
                                let src_idx = ((ratio * bars.len() as f32) as usize).min(bars.len() - 1);
                                (bars[src_idx].clamp(0.0, 1.0), peaks[src_idx].clamp(0.0, 1.0))
                            } else {
                                (0.0, 0.0)
                            };

                            let total_bar_height = val * (height as f32);
                            let peak_row = ((peak * height as f32).round() as usize).min(height.saturating_sub(1));

                            // 周波数帯域グラデーションカラー (低域: シアン, 中域: グリーン, 高域: ピンク)
                            let progress = bar_idx as f32 / num_bars.max(1) as f32;
                            let bar_color = if progress < 0.33 {
                                Color::Rgb(56, 189, 248) // Bass
                            } else if progress < 0.66 {
                                Color::Rgb(52, 211, 153) // Mid
                            } else {
                                Color::Rgb(244, 114, 182) // Treble
                            };

                            for row in 0..height {
                                let bottom_up = height - 1 - row;
                                let cell_x = rect.x + col as u16;
                                let cell_y = rect.y + row as u16;

                                if cell_x < rect.x + rect.width && cell_y < rect.y + rect.height {
                                    let cell = &mut buf[(cell_x, cell_y)];

                                    // ピークドット判定
                                    if bottom_up == peak_row && peak_row > total_bar_height.ceil() as usize && peak > 0.05 {
                                        cell.set_symbol("▔");
                                        cell.set_fg(Color::Rgb(255, 255, 255));
                                        cell.set_bg(self.theme.bg_card);
                                    } else if total_bar_height >= (bottom_up + 1) as f32 {
                                        cell.set_symbol("█");
                                        cell.set_fg(bar_color);
                                        cell.set_bg(self.theme.bg_card);
                                    } else if total_bar_height <= bottom_up as f32 {
                                        cell.set_symbol(" ");
                                        cell.set_bg(self.theme.bg_card);
                                    } else {
                                        let frac = total_bar_height - bottom_up as f32;
                                        let idx = ((frac * 8.0).round() as usize).clamp(1, 8);
                                        cell.set_symbol(&blocks[idx].to_string());
                                        cell.set_fg(bar_color);
                                        cell.set_bg(self.theme.bg_card);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
}
