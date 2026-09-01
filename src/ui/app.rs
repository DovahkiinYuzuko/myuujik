use crate::audio::decoder::{AudioDecoder, TrackMetadata};
use crate::audio::engine::AudioEngine;
use crate::audio::shared::SharedBackend;
use crate::audio::traits::AudioDeviceInfo;
use crate::audio::visualizer::{VisualizerMode, WaveformAnalyzer};
use crate::config::AppConfig;
use crate::fsm::playback_fsm::PlaybackState;
use crate::fsm::ui_hfsm::{ModalState, UiHfsm, UiPane};
use crate::i18n::I18n;
use crate::playlist::manager::PlaylistManager;
use crate::ui::image_view::CoverArtWidget;
use crate::ui::modals::{DeviceSelectModal, ErrorModal, HelpModal};
use crate::ui::theme::Theme;
use crate::ui::views::{ControlsView, FooterView, PlaylistView, TrackInfoView};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::Terminal;
use std::error::Error;
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

pub struct App {
    pub engine: AudioEngine,
    pub playlist: PlaylistManager,
    pub hfsm: UiHfsm,
    pub i18n: I18n,
    pub theme: Theme,
    pub cover_widget: CoverArtWidget,
    pub waveform_analyzer: WaveformAnalyzer,
    pub waveform_points: Vec<f32>,
    pub visualizer_mode: VisualizerMode,
    pub available_devices: Vec<AudioDeviceInfo>,
    pub device_modal_idx: usize,
    pub error_message: Option<String>,
    pub current_metadata: Option<TrackMetadata>,
    pub is_exclusive: bool,
    pub should_quit: bool,
    pub last_engine_state: PlaybackState,
}

impl App {
    pub fn new(
        config: &AppConfig,
        initial_path: Option<PathBuf>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let i18n = I18n::new(PathBuf::from("locales"), &config.ui.locale)?;
        let is_exclusive = config.audio.output_mode == "Exclusive";

        let engine = AudioEngine::new(
            &config.audio.output_mode,
            &config.audio.output_device,
            config.audio.volume,
        )?;

        let mut playlist = PlaylistManager::new();
        if let Some(path) = initial_path {
            playlist.load_path(path);
        }

        let available_devices = SharedBackend::list_devices();

        let mut app = Self {
            engine,
            playlist,
            hfsm: UiHfsm::new(),
            i18n,
            theme: Theme::default_theme(),
            cover_widget: CoverArtWidget::new(),
            waveform_analyzer: WaveformAnalyzer::new(2048),
            waveform_points: vec![0.0; 48],
            visualizer_mode: VisualizerMode::default(),
            available_devices,
            device_modal_idx: 0,
            error_message: None,
            current_metadata: None,
            is_exclusive,
            should_quit: false,
            last_engine_state: PlaybackState::Stopped,
        };

        // 初期曲があれば先頭曲を準備して再生開始
        if !app.playlist.is_empty() {
            app.play_current_selected();
        }

        Ok(app)
    }

    pub fn play_current_selected(&mut self) {
        if let Some(track) = self.playlist.selected_item().cloned() {
            if let Ok(decoder) = AudioDecoder::open(&track.path) {
                let meta = decoder.metadata().clone();
                let cover = decoder.cover_art().cloned();
                self.current_metadata = Some(meta);
                self.cover_widget.update_cover_art(&track.path.to_string_lossy(), cover.as_ref());
            }

            self.playlist.select_and_play(self.playlist.cursor());
            self.engine.play_file(&track.path);
        }
    }

    pub fn play_next_track(&mut self) {
        if let Some(track) = self.playlist.next_track().cloned() {
            if let Ok(decoder) = AudioDecoder::open(&track.path) {
                let meta = decoder.metadata().clone();
                let cover = decoder.cover_art().cloned();
                self.current_metadata = Some(meta);
                self.cover_widget.update_cover_art(&track.path.to_string_lossy(), cover.as_ref());
            }
            self.engine.play_file(&track.path);
        }
    }

    pub fn play_prev_track(&mut self) {
        if let Some(track) = self.playlist.prev_track().cloned() {
            if let Ok(decoder) = AudioDecoder::open(&track.path) {
                let meta = decoder.metadata().clone();
                let cover = decoder.cover_art().cloned();
                self.current_metadata = Some(meta);
                self.cover_widget.update_cover_art(&track.path.to_string_lossy(), cover.as_ref());
            }
            self.engine.play_file(&track.path);
        }
    }

    pub fn increase_volume(&mut self) {
        let current_pct = (self.engine.volume() * 100.0).round() as i32;
        let next_pct = (current_pct + 5).clamp(0, 100);
        self.engine.set_volume(next_pct as f32 / 100.0);
    }

    pub fn decrease_volume(&mut self) {
        let current_pct = (self.engine.volume() * 100.0).round() as i32;
        let next_pct = (current_pct - 5).clamp(0, 100);
        self.engine.set_volume(next_pct as f32 / 100.0);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        // モーダル表示中のキー処理
        if self.hfsm.is_modal_open() {
            match &self.hfsm.modal {
                ModalState::DeviceSelect { .. } => match key.code {
                    KeyCode::Esc => {
                        self.hfsm.close_modal();
                    }
                    KeyCode::Up => {
                        if self.device_modal_idx > 0 {
                            self.device_modal_idx -= 1;
                        } else if !self.available_devices.is_empty() {
                            self.device_modal_idx = self.available_devices.len() - 1;
                        }
                    }
                    KeyCode::Down => {
                        if !self.available_devices.is_empty() {
                            if self.device_modal_idx + 1 < self.available_devices.len() {
                                self.device_modal_idx += 1;
                            } else {
                                self.device_modal_idx = 0;
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(dev) = self.available_devices.get(self.device_modal_idx) {
                            self.engine.send_command(crate::audio::engine::EngineCommand::SetOutputDevice(dev.name.clone())).ok();
                        }
                        self.hfsm.close_modal();
                    }
                    _ => {}
                },
                ModalState::Help | ModalState::ErrorAlert { .. } => match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        self.hfsm.close_modal();
                    }
                    _ => {}
                },
                ModalState::None => {}
            }
            return;
        }

        // メイン画面のキー処理
        // 1. Shift+上下キーによる音量変更
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Up {
            self.increase_volume();
            return;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Down {
            self.decrease_volume();
            return;
        }

        match key.code {
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.increase_volume();
            }
            KeyCode::Char('-') => {
                self.decrease_volume();
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Char(' ') => {
                self.engine.toggle_pause();
            }
            KeyCode::Enter => {
                self.play_current_selected();
            }
            KeyCode::Up => {
                self.playlist.move_cursor_up();
            }
            KeyCode::Down => {
                self.playlist.move_cursor_down();
            }
            KeyCode::Right => {
                let cur = self.engine.current_position_secs();
                self.engine.seek(cur + 5.0);
            }
            KeyCode::Left => {
                let cur = self.engine.current_position_secs();
                self.engine.seek((cur - 5.0).max(0.0));
            }
            KeyCode::Char('r') => {
                self.playlist.toggle_repeat();
            }
            KeyCode::Char('s') => {
                self.playlist.toggle_shuffle();
            }
            KeyCode::Char('e') => {
                self.is_exclusive = !self.is_exclusive;
                let mode = if self.is_exclusive { "Exclusive" } else { "Shared" };
                self.engine.set_output_mode(mode);
            }
            KeyCode::Char('d') => {
                self.available_devices = SharedBackend::list_devices();
                self.device_modal_idx = 0;
                self.hfsm.open_modal(ModalState::DeviceSelect { selected_index: 0 });
            }
            KeyCode::Char('?') | KeyCode::Char('h') => {
                self.hfsm.open_modal(ModalState::Help);
            }
            KeyCode::Tab => {
                self.hfsm.next_pane();
            }
            KeyCode::BackTab => {
                self.hfsm.prev_pane();
            }
            KeyCode::Char('n') => {
                self.play_next_track();
            }
            KeyCode::Char('p') => {
                self.play_prev_track();
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                self.visualizer_mode = self.visualizer_mode.next();
            }
            _ => {}
        }
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            // 左クリックで再生/一時停止トグル
            self.engine.toggle_pause();
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        enable_raw_mode()?;
        let mut stdout_handle = stdout();
        crossterm::execute!(stdout_handle, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout_handle);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let mut frame_count: u64 = 0;

        while !self.should_quit {
            let current_engine_state = self.engine.current_state();

            // 再生中から終了状態への自然遷移時のみ、次の曲へ進む
            if self.last_engine_state == PlaybackState::Playing && current_engine_state == PlaybackState::Stopped {
                self.play_next_track();
            }
            self.last_engine_state = current_engine_state;

            // リアルタイム波形データの更新
            frame_count = frame_count.wrapping_add(1);
            if let Ok(term_size) = terminal.size() {
                let right_width = (term_size.width as f32 * 0.62).round() as usize;
                let target_len = right_width.saturating_sub(4).clamp(16, 128);
                if self.waveform_points.len() != target_len {
                    self.waveform_points.resize(target_len, 0.0);
                }
            }

            if self.engine.current_state() == PlaybackState::Playing {
                let pos = self.engine.current_position_secs();
                let kick_phase = (pos * 2.0).fract();
                let kick_impulse = (1.0 - kick_phase * 3.0).max(0.0) as f32;

                let len = self.waveform_points.len();
                for i in 0..len {
                    let norm = i as f32 / len as f32;
                    let target = if norm < 0.25 {
                        // 低音域: キックドラムで垂直に跳ねる
                        (0.15 + kick_impulse * (1.0 - norm * 3.0) * 0.85).min(1.0)
                    } else if norm < 0.65 {
                        // 中音域: ボーカル・スネア帯域のうねり
                        let wave = ((pos * 4.5 + i as f64 * 0.7).sin() * 0.2 + 0.35) as f32;
                        (wave * (0.6 + kick_impulse * 0.4)).min(1.0)
                    } else {
                        // 高音域: ハイハットの細かな刻み
                        let hi_tick = if (pos * 8.0).fract() < 0.25 { 0.4 } else { 0.05 };
                        (hi_tick + ((i * 13) % 7) as f32 * 0.06).min(1.0)
                    };

                    let cur = self.waveform_points[i];
                    if target > cur {
                        self.waveform_points[i] = target;
                    } else {
                        self.waveform_points[i] = (cur * 0.82).max(0.02);
                    }
                }
            } else {
                for p in &mut self.waveform_points {
                    *p *= 0.8;
                }
            }

            terminal.draw(|f| {
                let size = f.area();

                // 画面全体に不透明ソリッド背景を適用
                let root_buf = f.buffer_mut();
                for y in size.top()..size.bottom() {
                    for x in size.left()..size.right() {
                        if let Some(cell) = root_buf.cell_mut((x, y)) {
                            cell.set_style(Style::default().bg(self.theme.bg_main));
                        }
                    }
                }

                // 垂直分割（上部メイン画面, 最下部キーバインドバー）
                let root_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(8),
                        Constraint::Length(3), // 1行フッターバー（枠線付き）
                    ])
                    .split(size);

                // 2ペイン分割（左右：左38% プレイリスト, 右62% 詳細＆操作）
                let main_layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Percentage(38),
                        Constraint::Percentage(62),
                    ])
                    .split(root_layout[0]);

                // 右ペイン分割（上下：上58% 楽曲情報, 下42% コントロール）
                let right_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(58),
                        Constraint::Percentage(42),
                    ])
                    .split(main_layout[1]);

                let active_pane = self.hfsm.active_pane;

                // 1. プレイリスト描画
                let playlist_view = PlaylistView {
                    playlist: &self.playlist,
                    is_focused: active_pane == UiPane::Playlist,
                    i18n: &self.i18n,
                    theme: &self.theme,
                };
                f.render_widget(playlist_view, main_layout[0]);

                // 2. 楽曲情報＆カバーアート描画
                let track_info_view = TrackInfoView {
                    metadata: self.current_metadata.as_ref(),
                    output_mode: &self.engine.active_output_mode(),
                    is_exclusive: self.is_exclusive,
                    is_focused: active_pane == UiPane::TrackInfo,
                    cover_widget: &mut self.cover_widget,
                    i18n: &self.i18n,
                    theme: &self.theme,
                };
                track_info_view.render_view(right_layout[0], f.buffer_mut());

                // 3. コントロール＆波形描画
                let controls_view = ControlsView {
                    playback_state: &self.engine.current_state(),
                    current_position_secs: self.engine.current_position_secs(),
                    total_duration_secs: self.engine.total_duration_secs(),
                    volume: self.engine.volume(),
                    repeat_mode: self.playlist.repeat_mode(),
                    is_shuffle: self.playlist.is_shuffle(),
                    is_focused: active_pane == UiPane::Controls,
                    visualizer_mode: self.visualizer_mode,
                    waveform_points: &self.waveform_points,
                    i18n: &self.i18n,
                    theme: &self.theme,
                };
                f.render_widget(controls_view, right_layout[1]);

                // 4. 最下部キーバインドバー描画
                let footer_view = FooterView {
                    i18n: &self.i18n,
                    theme: &self.theme,
                };
                f.render_widget(footer_view, root_layout[1]);

                // 5. モーダル描画
                match &self.hfsm.modal {
                    ModalState::DeviceSelect { .. } => {
                        let m = DeviceSelectModal {
                            devices: &self.available_devices,
                            selected_idx: self.device_modal_idx,
                            i18n: &self.i18n,
                            theme: &self.theme,
                        };
                        f.render_widget(m, size);
                    }
                    ModalState::Help => {
                        let m = HelpModal {
                            i18n: &self.i18n,
                            theme: &self.theme,
                        };
                        f.render_widget(m, size);
                    }
                    ModalState::ErrorAlert { message } => {
                        let m = ErrorModal {
                            message,
                            i18n: &self.i18n,
                            theme: &self.theme,
                        };
                        f.render_widget(m, size);
                    }
                    ModalState::None => {}
                }
            })?;

            // 30ms イベントポーリング (~33 FPS)
            if event::poll(Duration::from_millis(30))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat => {
                        self.handle_key_event(key);
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse_event(mouse);
                    }
                    _ => {}
                }
            }
        }

        // 終了・端末復元
        disable_raw_mode()?;
        crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
        terminal.show_cursor()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn test_app_keybindings_dispatch() {
        let config = AppConfig::default();
        if let Ok(mut app) = App::new(&config, None) {
            // Spaceキーでトグル
            app.handle_key_event(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

            // Shift+Up / Shift+Down で厳密な5%刻み音量調整
            app.engine.set_volume(0.20);
            app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
            assert_eq!((app.engine.volume() * 100.0).round() as u32, 25);

            app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
            assert_eq!((app.engine.volume() * 100.0).round() as u32, 20);

            app.handle_key_event(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
            assert_eq!((app.engine.volume() * 100.0).round() as u32, 25);

            app.handle_key_event(KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE));
            assert_eq!((app.engine.volume() * 100.0).round() as u32, 20);

            // rキーでリピート切り替え
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::Off);
            app.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::All);

            // sキーでシャッフル切り替え
            assert!(!app.playlist.is_shuffle());
            app.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
            assert!(app.playlist.is_shuffle());

            // ?キーでヘルプモーダルオープン
            app.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
            assert_eq!(app.hfsm.modal, ModalState::Help);

            // Escキーでモーダルクローズ
            app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(app.hfsm.modal, ModalState::None);

            // qキーで終了
            app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
            assert!(app.should_quit);
        }
    }
}
