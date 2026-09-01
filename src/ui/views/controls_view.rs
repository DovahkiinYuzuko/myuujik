use crate::audio::visualizer::VisualizerMode;
use crate::fsm::playback_fsm::PlaybackState;
use crate::i18n::I18n;
use crate::playlist::manager::RepeatMode;
use crate::ui::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};
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
            PlaybackState::Playing => "● PLAYING",
            PlaybackState::Paused => "❚❚ PAUSED",
            PlaybackState::Stopped => "■ STOPPED",
            PlaybackState::Buffering { .. } => "⟳ BUFFERING",
            PlaybackState::Seeking { .. } => "⟲ SEEKING",
            PlaybackState::TrackChanging { .. } => "⟳ CHANGING",
            PlaybackState::Error { .. } => "▲ ERROR",
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

        // 2. ステータス行
        let repeat_str = match self.repeat_mode {
            RepeatMode::Off => "LOOP: OFF",
            RepeatMode::All => "LOOP: ALL",
            RepeatMode::Single => "LOOP: 1",
        };

        let shuffle_str = if self.is_shuffle {
            "SHUF: ON"
        } else {
            "SHUF: OFF"
        };

        let vol_percent = (self.volume * 100.0).round() as u32;
        let vol_str = format!("VOL: {}%", vol_percent);

        let status_spans = vec![
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
                format!(" [ {} ] ", self.visualizer_mode.display_name()),
                Style::default().fg(Color::Rgb(56, 189, 248)).bg(Color::Rgb(20, 28, 45)).add_modifier(Modifier::BOLD),
            ),
        ];

        let status_para = Paragraph::new(Line::from(status_spans)).style(Style::default().bg(self.theme.bg_card));
        status_para.render(chunks[1], buf);

        // 3. ビジュアライザ描画 (AviUtl Type 3 / Type 4 / Type 3 Polar)
        if chunks[2].height > 0 && chunks[2].width > 0 {
            match self.visualizer_mode {
                VisualizerMode::Type3 => {
                    // METER: Unicode ブロック文字による複数行バーグラフ (底辺接地・高さフル活用)
                    let blocks = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                    let width = chunks[2].width as usize;
                    let height = chunks[2].height as usize;

                    let mut lines = Vec::with_capacity(height);
                    for row in 0..height {
                        // row 0 が一番上、row (height - 1) が一番下 (底辺接地)
                        let bottom_up = height - 1 - row;
                        let mut line_chars = String::with_capacity(width);

                        for col in 0..width {
                            let val = if col < self.waveform_points.len() {
                                self.waveform_points[col].clamp(0.0, 1.0)
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
                    // Type 4: 波状の音量メーター (スパークライン波形)
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
                VisualizerMode::Type3Polar => {
                    // Type 3 極座標変換 (Ratatui Canvas 点字 Braille による円形サークル波形)
                    let points_count = 48.min(self.waveform_points.len()).max(16);
                    let mut lines = Vec::with_capacity(points_count);
                    let r_inner = 16.0f64;
                    let r_max = 38.0f64;

                    for i in 0..points_count {
                        let val = if i < self.waveform_points.len() {
                            self.waveform_points[i] as f64
                        } else {
                            0.0
                        };
                        let angle = (i as f64 / points_count as f64) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
                        let r1 = r_inner;
                        let r2 = r_inner + val * (r_max - r_inner);

                        let cos_a = angle.cos();
                        let sin_a = angle.sin();

                        lines.push((
                            cos_a * r1,
                            sin_a * r1,
                            cos_a * r2,
                            sin_a * r2,
                        ));
                    }

                    let canvas = Canvas::default()
                        .block(Block::default().style(Style::default().bg(self.theme.bg_card)))
                        .x_bounds([-45.0, 45.0])
                        .y_bounds([-45.0, 45.0])
                        .paint(move |ctx| {
                            ctx.draw(&Circle {
                                x: 0.0,
                                y: 0.0,
                                radius: r_inner,
                                color: Color::Rgb(56, 189, 248),
                            });
                            for &(x1, y1, x2, y2) in &lines {
                                ctx.draw(&CanvasLine {
                                    x1,
                                    y1,
                                    x2,
                                    y2,
                                    color: Color::White,
                                });
                            }
                        });
                    canvas.render(chunks[2], buf);
                }
            }
        }
    }
}
