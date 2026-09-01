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
use ratatui::layout::{Constraint, Direction, Layout, Rect};
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
    pub playlist_area: Rect,
    pub track_info_area: Rect,
    pub controls_area: Rect,
    pub progress_area: Rect,
    pub status_area: Rect,
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
            playlist_area: Rect::default(),
            track_info_area: Rect::default(),
            controls_area: Rect::default(),
            progress_area: Rect::default(),
            status_area: Rect::default(),
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
                let target_mode = if self.is_exclusive { "Shared" } else { "Exclusive" };
                self.engine.set_output_mode(target_mode);
                self.is_exclusive = target_mode == "Exclusive";
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

    fn is_inside_rect(x: u16, y: u16, rect: Rect) -> bool {
        x >= rect.x && x < rect.x.saturating_add(rect.width) && y >= rect.y && y < rect.y.saturating_add(rect.height)
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let col = mouse.column;
                let row = mouse.row;

                // 1. プログレスバーのクリック -> シーク
                if Self::is_inside_rect(col, row, self.progress_area) && self.progress_area.width > 0 {
                    let offset = col.saturating_sub(self.progress_area.x) as f64;
                    let ratio = (offset / self.progress_area.width as f64).clamp(0.0, 1.0);
                    let total = self.engine.total_duration_secs();
                    if total > 0.0 {
                        self.engine.seek(ratio * total);
                    }
                    return;
                }

                // 2. ステータス行のクリック -> 各種バッジのトグル
                if Self::is_inside_rect(col, row, self.status_area) {
                    let offset_x = col.saturating_sub(self.status_area.x);
                    if offset_x < 15 {
                        self.engine.toggle_pause();
                    } else if offset_x < 28 {
                        self.playlist.toggle_repeat();
                    } else if offset_x < 41 {
                        self.playlist.toggle_shuffle();
                    } else if offset_x < 54 {
                        self.increase_volume();
                    } else {
                        self.visualizer_mode = self.visualizer_mode.next();
                    }
                    return;
                }

                // 3. 楽曲情報ペインのクリック -> 出力モード（Shared/Exclusive）の切り替え
                if Self::is_inside_rect(col, row, self.track_info_area) {
                    let target_mode = if self.is_exclusive { "Shared" } else { "Exclusive" };
                    self.engine.set_output_mode(target_mode);
                    self.is_exclusive = target_mode == "Exclusive";
                    return;
                }

                // 4. プレイリスト領域のクリック -> 楽曲選択＆即時再生
                if Self::is_inside_rect(col, row, self.playlist_area) {
                    let inner_y = self.playlist_area.y.saturating_add(1);
                    let inner_bottom = self.playlist_area.y.saturating_add(self.playlist_area.height).saturating_sub(1);
                    if row >= inner_y && row < inner_bottom {
                        let clicked_line = (row - inner_y) as usize;
                        if clicked_line < self.playlist.len() {
                            self.playlist.set_cursor(clicked_line);
                            self.play_current_selected();
                        }
                    }
                    return;
                }

                // 5. その他の領域（空白領域など） -> 従来通り再生/一時停止トグル
                self.engine.toggle_pause();
            }
            MouseEventKind::Down(MouseButton::Right) => {
                let col = mouse.column;
                let row = mouse.row;
                // 右クリックで音量ダウン（コントロール領域またはステータス行）
                if Self::is_inside_rect(col, row, self.controls_area) {
                    self.decrease_volume();
                }
            }
            MouseEventKind::ScrollUp => {
                let col = mouse.column;
                let row = mouse.row;
                if Self::is_inside_rect(col, row, self.playlist_area) {
                    self.playlist.move_cursor_up();
                } else {
                    self.increase_volume();
                }
            }
            MouseEventKind::ScrollDown => {
                let col = mouse.column;
                let row = mouse.row;
                if Self::is_inside_rect(col, row, self.playlist_area) {
                    self.playlist.move_cursor_down();
                } else {
                    self.decrease_volume();
                }
            }
            _ => {}
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
                match self.visualizer_mode {
                    VisualizerMode::Type4 => {
                        // WAVE: 滑らかにスルスルうねる多重サイン波オシロスコープ
                        let pos = self.engine.current_position_secs();
                        let len = self.waveform_points.len();
                        for i in 0..len {
                            let t = pos * 8.0 + (i as f64 * 0.35);
                            let val = ((t.sin() * 0.4 + (t * 2.3).sin() * 0.3 + 0.5).abs() as f32).clamp(0.05, 1.0);
                            self.waveform_points[i] = val;
                        }
                    }
                    VisualizerMode::Type3 => {
                        // METER: スピーカー出力直結のゼロレイテンシ本物PCMサンプル
                        let raw_points = self.engine.get_waveform_points(self.waveform_points.len());
                        for (cur, &new_val) in self.waveform_points.iter_mut().zip(raw_points.iter()) {
                            if new_val > *cur {
                                *cur = new_val;
                            } else {
                                *cur = (*cur * 0.80 + new_val * 0.20).max(0.01);
                            }
                        }
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

                // マウス判定用に対象領域の矩形をキャッシュ
                self.playlist_area = main_layout[0];
                self.track_info_area = right_layout[0];
                self.controls_area = right_layout[1];

                let controls_inner = Rect {
                    x: self.controls_area.x.saturating_add(1),
                    y: self.controls_area.y.saturating_add(1),
                    width: self.controls_area.width.saturating_sub(2),
                    height: self.controls_area.height.saturating_sub(2),
                };

                let control_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1), // プログレスバー
                        Constraint::Length(1), // ステータス行
                        Constraint::Min(2),    // リアルタイム波形スパークライン
                    ])
                    .split(controls_inner);

                self.progress_area = control_chunks[0];
                self.status_area = control_chunks[1];

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
                let is_fallback = self.engine.is_fallback();
                let active_mode = self.engine.active_output_mode();
                if is_fallback {
                    self.is_exclusive = false;
                } else {
                    self.is_exclusive = active_mode.starts_with("Exclusive");
                }

                let track_info_view = TrackInfoView {
                    metadata: self.current_metadata.as_ref(),
                    output_mode: &active_mode,
                    is_exclusive: self.is_exclusive,
                    is_fallback,
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

    #[test]
    fn test_app_mouse_controls() {
        let config = AppConfig::default();
        if let Ok(mut app) = App::new(&config, None) {
            // 領域の設定
            app.playlist_area = Rect::new(0, 0, 30, 20);
            app.progress_area = Rect::new(32, 12, 40, 1);
            app.status_area = Rect::new(32, 13, 40, 1);
            app.track_info_area = Rect::new(32, 0, 40, 10);
            app.controls_area = Rect::new(32, 11, 40, 9);

            // 1. ホイール上下による音量制御テスト
            let initial_vol = app.engine.volume();
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 35,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.engine.volume() >= initial_vol);

            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 35,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });

            // 2. ステータス行クリックによるリピートトグルテスト (offset 16: LOOPバッジ)
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::Off);
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 18,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::All);

            // 3. ステータス行クリックによるシャッフルトグルテスト (offset 30: SHUFバッジ)
            assert!(!app.playlist.is_shuffle());
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 30,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.playlist.is_shuffle());

            // 4. トラック情報領域クリックによる出力モードトグルテスト
            let initial_mode = app.is_exclusive;
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 35,
                row: 5,
                modifiers: KeyModifiers::NONE,
            });
            assert_ne!(app.is_exclusive, initial_mode);
        }
    }
}
