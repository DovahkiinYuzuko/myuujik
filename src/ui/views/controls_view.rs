use crate::fsm::playback_fsm::PlaybackState;
use crate::i18n::I18n;
use crate::playlist::manager::RepeatMode;
use crate::ui::theme::Theme;
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
        ];

        let status_para = Paragraph::new(Line::from(status_spans)).style(Style::default().bg(self.theme.bg_card));
        status_para.render(chunks[1], buf);

        // 3. リアルタイム波形スパークライン
        if chunks[2].height > 0 {
            let spark_data: Vec<u64> = self
                .waveform_points
                .iter()
                .map(|&p| (p * 100.0).clamp(0.0, 100.0) as u64)
                .collect();

            let sparkline = Sparkline::default()
                .style(Style::default().fg(self.theme.primary).bg(self.theme.bg_card))
                .data(&spark_data)
                .max(100);
            sparkline.render(chunks[2], buf);
        }
    }
}
