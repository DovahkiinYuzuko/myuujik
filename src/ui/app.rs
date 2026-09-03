use crate::audio::decoder::{AudioDecoder, TrackMetadata};
use crate::audio::engine::{AudioEngine, EngineNotification};
use crate::audio::shared::SharedBackend;
use crate::audio::traits::AudioDeviceInfo;
use crate::audio::visualizer::{FftSpectrumAnalyzer, VisualizerMode, WaveformAnalyzer};
use crate::config::AppConfig;
use crate::fsm::playback_fsm::PlaybackState;
use crate::fsm::ui_hfsm::{ModalState, UiHfsm, UiPane};
use crate::i18n::I18n;
use crate::playlist::item::PlaylistEntry;
use crate::playlist::manager::PlaylistManager;
use crate::ui::image_view::CoverArtWidget;
use crate::ui::modals::{DeviceSelectModal, EqualizerModal, ErrorModal, HelpModal};
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
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub struct App {
    pub engine: AudioEngine,
    pub playlist: PlaylistManager,
    pub hfsm: UiHfsm,
    pub i18n: I18n,
    pub theme: Theme,
    pub cover_widget: CoverArtWidget,
    pub waveform_analyzer: WaveformAnalyzer,
    pub waveform_points: Vec<f32>,
    pub spectrum_analyzer: FftSpectrumAnalyzer,
    pub visualizer_mode: VisualizerMode,
    pub available_devices: Vec<AudioDeviceInfo>,
    pub device_modal_idx: usize,
    pub error_message: Option<String>,
    pub current_metadata: Option<TrackMetadata>,
    pub pending_cd_disc_id: Option<(String, String)>,
    pub is_exclusive: bool,
    pub should_quit: bool,
    pub last_engine_state: PlaybackState,
    pub playlist_area: Rect,
    pub track_info_area: Rect,
    pub controls_area: Rect,
    pub progress_area: Rect,
    pub status_area: Rect,
    pub is_dragging_seekbar: bool,
    pub drag_target_secs: Option<f64>,
    pub cursor_moved_at: Instant,
    pub track_changed_at: Instant,
    pub folder_picker_rx: Option<Receiver<Option<PathBuf>>>,
    pub config: AppConfig,
    pub is_searching: bool,
    pub search_query: String,
    pub current_lyrics: Option<crate::audio::lyrics::Lyrics>,
    pub show_lyrics: bool,
    pub lyrics_fetch_rx: Option<Receiver<Result<(PathBuf, String), String>>>,
    pub lyrics_toast: Option<(String, Instant, bool)>,
    pub eq_enabled: bool,
    pub eq_gains: [f32; 10],
    pub eq_preset: String,
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
        match config.playback.repeat_mode.as_str() {
            "None" | "Off" => playlist.set_repeat_mode(crate::playlist::RepeatMode::Off),
            "Single" => playlist.set_repeat_mode(crate::playlist::RepeatMode::Single),
            _ => playlist.set_repeat_mode(crate::playlist::RepeatMode::All),
        }
        playlist.set_shuffle(config.playback.shuffle);

        if let Some(path) = initial_path {
            playlist.load_path(path);
        }

        let available_devices = SharedBackend::list_devices();
        let visualizer_mode = match config.ui.visualizer_mode.as_str() {
            "Type3" => VisualizerMode::Type3,
            "Type4" => VisualizerMode::Type4,
            "Spectrum" => VisualizerMode::Spectrum,
            _ => VisualizerMode::default(),
        };

        let mut app = Self {
            engine,
            playlist,
            hfsm: UiHfsm::new(),
            i18n,
            theme: Theme::default_theme(),
            cover_widget: CoverArtWidget::new(),
            waveform_analyzer: WaveformAnalyzer::new(2048),
            waveform_points: vec![0.0; 48],
            spectrum_analyzer: FftSpectrumAnalyzer::default(),
            visualizer_mode,
            available_devices,
            device_modal_idx: 0,
            error_message: None,
            current_metadata: None,
            pending_cd_disc_id: None,
            is_exclusive,
            should_quit: false,
            last_engine_state: PlaybackState::Stopped,
            playlist_area: Rect::default(),
            track_info_area: Rect::default(),
            controls_area: Rect::default(),
            progress_area: Rect::default(),
            status_area: Rect::default(),
            is_dragging_seekbar: false,
            drag_target_secs: None,
            cursor_moved_at: Instant::now(),
            track_changed_at: Instant::now(),
            folder_picker_rx: None,
            config: config.clone(),
            is_searching: false,
            search_query: String::new(),
            current_lyrics: None,
            show_lyrics: config.ui.show_lyrics,
            lyrics_fetch_rx: None,
            lyrics_toast: None,
            eq_enabled: config.equalizer.enabled,
            eq_gains: {
                let mut g = [0.0f32; 10];
                for (i, &val) in config.equalizer.gains.iter().take(10).enumerate() {
                    g[i] = val;
                }
                g
            },
            eq_preset: config.equalizer.preset.clone(),
        };

        app.engine.set_equalizer_enabled(app.eq_enabled);
        app.engine.set_equalizer_gains(app.eq_gains.to_vec());
        app.engine.set_normalize_loudness(app.config.playback.normalize_loudness);
        app.engine.set_crossfade_secs(app.config.playback.crossfade_secs);

        let target_track = if let Some(ref saved_path_str) = config.session.last_track_path {
            let p = PathBuf::from(saved_path_str);
            app.playlist.all_tracks().iter().find(|t| t.path == p).cloned()
        } else {
            None
        }.or_else(|| {
            app.playlist.all_tracks().get(config.session.last_track_index).cloned()
        }).or_else(|| {
            app.playlist.all_tracks().first().cloned()
        });

        if let Some(track) = target_track {
            app.playlist.select_and_play_path(&track.path);
            app.apply_track_playback(&track.path);

            if config.session.last_position_secs > 0.0 {
                app.engine.seek(config.session.last_position_secs);
                app.engine.pause();
            }
        }

        Ok(app)
    }

    pub fn play_current_selected(&mut self) {
        let cursor = self.playlist.cursor();
        if let Some(entry) = self.playlist.entries().get(cursor) {
            if let Some(audio) = entry.audio_item() {
                if self.playlist.current_track_path() == Some(&audio.path) {
                    match self.engine.current_state() {
                        PlaybackState::Playing => return, // すでに再生中なら何もしない（頭出しリセット防止）
                        PlaybackState::Paused => {
                            self.engine.resume();
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }

        if let Some(track) = self.playlist.select_and_play_entry(cursor).cloned() {
            self.apply_track_playback(&track.path);
        }
    }

    pub fn play_next_track(&mut self) {
        if let Some(track) = self.playlist.next_track().cloned() {
            self.apply_track_playback(&track.path);
        }
    }

    pub fn play_prev_track(&mut self) {
        if let Some(track) = self.playlist.prev_track().cloned() {
            self.apply_track_playback(&track.path);
        }
    }

    /// トラックパスからのデコーダ生成、メタデータ/カバーアート取得、および再生開始の共通処理
    pub fn apply_track_playback(&mut self, path: &std::path::Path) {
        self.current_lyrics = crate::audio::lyrics::load_for_track(path);
        if let Ok(decoder) = AudioDecoder::open(path) {
            let meta = decoder.metadata().clone();
            let cover = decoder.cover_art().cloned();
            let disc_id = decoder.disc_id().map(|s| s.to_string());
            let path_str = path.to_string_lossy().to_string();

            if let Some(did) = disc_id {
                let has_cover = cover.is_some();
                let has_meta = crate::audio::cd::metadata::load_cached_cd_metadata(&did).is_some();
                if !has_cover || !has_meta {
                    self.pending_cd_disc_id = Some((path_str.clone(), did));
                } else {
                    self.pending_cd_disc_id = None;
                }
            } else {
                self.pending_cd_disc_id = None;
            }

            self.current_metadata = Some(meta);
            self.cover_widget.update_cover_art(&path_str, cover.as_ref());
            self.track_changed_at = Instant::now();
        }
        self.engine.play_file(path);
    }

    /// ギャップレス再生で次曲へ遷移した際の UI 状態（メタデータ・カバーアート・歌詞）同期処理
    pub fn apply_gapless_track_transition(&mut self, path: &std::path::Path, meta: TrackMetadata) {
        self.playlist.select_and_play_path(path);
        self.current_lyrics = crate::audio::lyrics::load_for_track(path);
        self.current_metadata = Some(meta);

        let cover = if let Ok(decoder) = AudioDecoder::open(path) {
            decoder.cover_art().cloned()
        } else {
            None
        };
        let path_str = path.to_string_lossy().to_string();
        self.cover_widget.update_cover_art(&path_str, cover.as_ref());
        self.track_changed_at = Instant::now();
        self.cursor_moved_at = Instant::now();
    }

    /// CD カバーアートおよびメタデータの非同期ダウンロード完了を検知して UI に反映する
    pub fn check_pending_cover_art(&mut self) {
        if let Some((track_key, disc_id)) = &self.pending_cd_disc_id {
            let mut cover_resolved = false;
            let mut meta_resolved = false;

            // 1. カバーアートの反映
            if let Some(cache_path) = crate::audio::cd::metadata::get_cached_cover_art_path(disc_id) {
                if cache_path.exists() {
                    if let Ok(data) = std::fs::read(&cache_path) {
                        if !data.is_empty() {
                            let cover = crate::audio::decoder::CoverArt {
                                mime_type: "image/jpeg".to_string(),
                                data,
                            };
                            self.cover_widget.update_cover_art(track_key, Some(&cover));
                            cover_resolved = true;
                        }
                    }
                }
            }

            // 2. メタデータ（タイトル、アーティスト、アルバム）の遅延反映
            if let Some(cd_meta) = crate::audio::cd::metadata::load_cached_cd_metadata(disc_id) {
                if let Some(meta) = self.current_metadata.as_mut() {
                    if meta.codec_name == "CD-DA" {
                        meta.album = Some(cd_meta.album_title.clone());
                        if !cd_meta.artist.is_empty() {
                            meta.artist = Some(cd_meta.artist.clone());
                        }

                        // パスからトラック番号を抽出（例: "Track10.cda" -> 10）
                        let file_name = std::path::Path::new(track_key)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        let digits: String = file_name.chars().filter(|c| c.is_ascii_digit()).collect();
                        if let Ok(track_num) = digits.parse::<u8>() {
                            if let Some(t) = cd_meta.tracks.iter().find(|t| t.track_number == track_num) {
                                if !t.title.is_empty() {
                                    meta.title = Some(t.title.clone());
                                }
                                if let Some(track_artist) = &t.artist {
                                    if !track_artist.is_empty() {
                                        meta.artist = Some(track_artist.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                meta_resolved = true;
            }

            if cover_resolved && meta_resolved {
                self.pending_cd_disc_id = None;
            }
        }
    }

    /// 音響指紋を用いた歌詞自動取得をバックグラウンドスレッドで起動する
    pub fn start_lyrics_auto_fetch(&mut self) {
        if self.lyrics_fetch_rx.is_some() {
            return;
        }

        // 1. 再生中トラックがあればそれを対象に、なければプレイリストで選択中のトラックを対象にする
        let track_path = self.playlist.current_track()
            .map(|t| t.path.clone())
            .or_else(|| self.playlist.selected_item().map(|t| t.path.clone()));

        let track_path = if let Some(p) = track_path {
            p
        } else {
            return;
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.lyrics_fetch_rx = Some(rx);
        self.show_lyrics = true; // 歌詞表示ビューへ自動切り替え
        self.lyrics_toast = Some((
            self.i18n.t("lyrics.fetching").to_string(),
            Instant::now(),
            false,
        ));

        std::thread::spawn(move || {
            let res = crate::audio::lyrics_fetcher::auto_fetch_and_save_lyrics(&track_path);
            let _ = tx.send(res);
        });
    }

    /// バックグラウンド歌詞自動取得の完了を検知して UI に反映する
    pub fn check_lyrics_fetch_result(&mut self) {
        if let Some(ref rx) = self.lyrics_fetch_rx {
            if let Ok(res) = rx.try_recv() {
                self.lyrics_fetch_rx = None;
                match res {
                    Ok((lrc_path, track_title)) => {
                        let msg = self.i18n.t_args("lyrics.fetch_success", &[("track", &track_title)]);
                        self.lyrics_toast = Some((msg, Instant::now(), false));
                        self.show_lyrics = true;

                        // 現在再生中のトラックまたは選択中のトラックの歌詞であれば即座に読み込む
                        let current_path = self.playlist.current_track()
                            .map(|t| t.path.clone())
                            .or_else(|| self.playlist.selected_item().map(|t| t.path.clone()));

                        if let Some(target_p) = current_path {
                            if target_p.with_extension("lrc") == lrc_path {
                                if let Some(lyrics) = crate::audio::lyrics::load_for_track(&target_p) {
                                    self.current_lyrics = Some(lyrics);
                                }
                            }
                        }
                    }
                    Err(err) => {
                        let msg = self.i18n.t_args("lyrics.fetch_failed", &[("error", &err)]);
                        self.lyrics_toast = Some((msg, Instant::now(), true));
                    }
                }
            }
        }

        // トースト通知の自動消去（6秒経過で消去）
        if let Some((_, start, _)) = self.lyrics_toast {
            if start.elapsed().as_secs() >= 6 {
                self.lyrics_toast = None;
            }
        }
    }

    /// 現在再生中（または選択中）のトラックの歌詞ファイル（.lrc）を削除し、表示をクリアする
    pub fn delete_current_lyrics(&mut self) {
        let track_path = self.playlist.current_track()
            .map(|t| t.path.clone())
            .or_else(|| self.playlist.selected_item().map(|t| t.path.clone()));

        let track_path = if let Some(p) = track_path {
            p
        } else {
            return;
        };

        let lrc_path = track_path.with_extension("lrc");
        if lrc_path.exists() {
            if let Err(e) = std::fs::remove_file(&lrc_path) {
                let msg = self.i18n.t_args("lyrics.delete_failed", &[("error", &e.to_string())]);
                self.lyrics_toast = Some((msg, Instant::now(), true));
                return;
            }
        }

        self.current_lyrics = None;
        let msg = self.i18n.t("lyrics.deleted").to_string();
        self.lyrics_toast = Some((msg, Instant::now(), false));
    }

    /// OS標準のフォルダ選択ダイアログをバックグラウンドスレッドで起動する
    pub fn open_folder_picker(&mut self) {
        if self.folder_picker_rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.folder_picker_rx = Some(rx);

        std::thread::spawn(move || {
            let res = crate::ui::dialog::pick_folder();
            let _ = tx.send(res);
        });
    }

    /// フォルダ選択ダイアログの結果をポーリングしてプレイリストに反映する
    pub fn check_folder_picker_result(&mut self) {
        if let Some(ref rx) = self.folder_picker_rx {
            if let Ok(result) = rx.try_recv() {
                self.folder_picker_rx = None;
                if let Some(path) = result {
                    crate::logger::info("App", &format!("Folder picker selected path: {:?}", path));
                    let count = self.playlist.load_path(&path);
                    if count > 0 {
                        if let Some(first_track) = self.playlist.all_tracks().first().cloned() {
                            self.playlist.select_and_play_path(&first_track.path);
                            self.apply_track_playback(&first_track.path);
                        }
                    }
                }
            }
        }
    }

    /// 現在のプレイリストをカレントディレクトリに myuujik_playlist.m3u8 としてエクスポート保存する
    pub fn export_current_playlist(&mut self) {
        let target_dir = self
            .playlist
            .current_dir()
            .or_else(|| self.playlist.root_path())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let export_path = target_dir.join("myuujik_playlist.m3u8");
        let track_count = self.playlist.all_tracks().len();

        if track_count == 0 {
            return;
        }

        match self.playlist.export_m3u(&export_path) {
            Ok(count) => {
                let msg = self.i18n.t_args("playlist.exported", &[
                    ("path", "myuujik_playlist.m3u8"),
                    ("count", &count.to_string()),
                ]);
                self.lyrics_toast = Some((msg, Instant::now(), false));
            }
            Err(e) => {
                let msg = self.i18n.t_args("playlist.export_failed", &[
                    ("error", &e.to_string()),
                ]);
                self.lyrics_toast = Some((msg, Instant::now(), true));
            }
        }
    }

    /// 現在の再生状態・設定を AppConfig に反映して保存する
    pub fn save_session(&mut self) -> std::io::Result<()> {
        self.config.audio.volume = self.engine.volume();
        self.config.audio.output_mode = if self.is_exclusive {
            "Exclusive".to_string()
        } else {
            "Shared".to_string()
        };
        self.config.playback.repeat_mode = match self.playlist.repeat_mode() {
            crate::playlist::RepeatMode::Off => "None".to_string(),
            crate::playlist::RepeatMode::All => "All".to_string(),
            crate::playlist::RepeatMode::Single => "Single".to_string(),
        };
        self.config.playback.shuffle = self.playlist.is_shuffle();
        self.config.ui.visualizer_mode = match self.visualizer_mode {
            VisualizerMode::Type3 => "Type3".to_string(),
            VisualizerMode::Type4 => "Type4".to_string(),
            VisualizerMode::Spectrum => "Spectrum".to_string(),
        };
        self.config.ui.show_lyrics = self.show_lyrics;
        self.config.equalizer.enabled = self.eq_enabled;
        self.config.equalizer.gains = self.eq_gains.to_vec();
        self.config.equalizer.preset = self.eq_preset.clone();

        if let Some(root_path) = self.playlist.root_path() {
            self.config.session.last_opened_path = Some(root_path.to_string_lossy().to_string());
        }
        self.config.session.last_track_index = self.playlist.cursor();
        if let Some(entry) = self.playlist.selected_entry() {
            if let Some(audio) = entry.audio_item() {
                self.config.session.last_track_path = Some(audio.path.to_string_lossy().to_string());
            }
        }
        self.config.session.last_position_secs = self.engine.current_position_secs();

        crate::logger::info("App", &format!("Saving session config: {:?}", self.config.session));
        self.config.save()
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
                            self.config.audio.output_device = dev.name.clone();
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
                ModalState::Equalizer { selected_band } => {
                    let mut band = *selected_band;
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('g') | KeyCode::Char('G') => {
                            self.hfsm.close_modal();
                        }
                        KeyCode::Left => {
                            if band > 0 {
                                band -= 1;
                            } else {
                                band = 9;
                            }
                            self.hfsm.modal = ModalState::Equalizer { selected_band: band };
                        }
                        KeyCode::Right => {
                            if band < 9 {
                                band += 1;
                            } else {
                                band = 0;
                            }
                            self.hfsm.modal = ModalState::Equalizer { selected_band: band };
                        }
                        KeyCode::Up => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) { 2.0 } else { 0.5 };
                            self.eq_gains[band] = (self.eq_gains[band] + step).min(12.0);
                            self.eq_preset = "Custom".to_string();
                            self.engine.set_equalizer_gains(self.eq_gains.to_vec());
                        }
                        KeyCode::Down => {
                            let step = if key.modifiers.contains(KeyModifiers::SHIFT) { 2.0 } else { 0.5 };
                            self.eq_gains[band] = (self.eq_gains[band] - step).max(-12.0);
                            self.eq_preset = "Custom".to_string();
                            self.engine.set_equalizer_gains(self.eq_gains.to_vec());
                        }
                        KeyCode::Char(' ') => {
                            self.eq_enabled = !self.eq_enabled;
                            self.engine.set_equalizer_enabled(self.eq_enabled);
                        }
                        KeyCode::Char(c) if ('0'..='6').contains(&c) => {
                            let presets = crate::audio::equalizer::EqPreset::all();
                            let idx = (c as usize) - ('0' as usize);
                            if let Some(&p) = presets.get(idx) {
                                self.eq_gains = p.gains();
                                self.eq_preset = p.display_name().to_string();
                                self.engine.set_equalizer_gains(self.eq_gains.to_vec());
                            }
                        }
                        _ => {}
                    }
                }
                ModalState::None => {}
            }
            return;
        }

        // 検索入力モード中のキー処理
        if self.is_searching {
            match key.code {
                KeyCode::Esc => {
                    self.is_searching = false;
                    self.search_query.clear();
                    self.playlist.clear_filter();
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.playlist.selected_entry().cloned() {
                        if let Some(audio) = entry.audio_item() {
                            self.playlist.select_and_play_path(&audio.path);
                            self.apply_track_playback(&audio.path);
                        }
                    }
                    self.is_searching = false;
                    self.search_query.clear();
                    self.playlist.clear_filter();
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.playlist.set_filter(&self.search_query);
                }
                KeyCode::Up => {
                    self.playlist.move_cursor_up();
                    self.cursor_moved_at = Instant::now();
                }
                KeyCode::Down => {
                    self.playlist.move_cursor_down();
                    self.cursor_moved_at = Instant::now();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.playlist.set_filter(&self.search_query);
                }
                _ => {}
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

        // 2. Shift+左右キーによる曲スキップ（前の曲 / 次の曲）
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Left {
            self.play_prev_track();
            return;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::Right {
            self.play_next_track();
            return;
        }

        // 3. Alt+上下キーによるプレイリスト選択曲の手動並び替え
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Up {
            if self.playlist.move_item_up() {
                self.cursor_moved_at = Instant::now();
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Down {
            if self.playlist.move_item_down() {
                self.cursor_moved_at = Instant::now();
            }
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
                if let Some(entry) = self.playlist.selected_entry().cloned() {
                    match entry {
                        PlaylistEntry::ParentDir => {
                            self.playlist.go_to_parent();
                            self.cursor_moved_at = Instant::now();
                        }
                        PlaylistEntry::Directory { path, .. } => {
                            self.playlist.enter_directory(&path);
                            self.cursor_moved_at = Instant::now();
                        }
                        PlaylistEntry::AudioFile(_) => {
                            self.play_current_selected();
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if self.playlist.go_to_parent() {
                    self.cursor_moved_at = Instant::now();
                }
            }
            KeyCode::Up => {
                self.playlist.move_cursor_up();
                self.cursor_moved_at = Instant::now();
            }
            KeyCode::Down => {
                self.playlist.move_cursor_down();
                self.cursor_moved_at = Instant::now();
            }
            KeyCode::Right => {
                let cur = self.engine.current_position_secs();
                let total = self.engine.total_duration_secs();
                if total > 0.0 && cur + 5.0 >= total {
                    self.play_next_track();
                } else {
                    self.engine.seek(cur + 5.0);
                }
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
            KeyCode::Char('S') => {
                self.export_current_playlist();
            }
            KeyCode::Char('/') => {
                self.is_searching = true;
                self.search_query.clear();
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                self.open_folder_picker();
            }
            KeyCode::Char('e') => {
                let target_mode = if self.is_exclusive { "Shared" } else { "Exclusive" };
                self.engine.set_output_mode(target_mode);
                self.is_exclusive = target_mode == "Exclusive";
            }
            KeyCode::Char('E') => {
                self.available_devices = SharedBackend::list_devices();
                self.device_modal_idx = 0;
                self.hfsm.open_modal(ModalState::DeviceSelect { selected_index: 0 });
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                self.hfsm.open_modal(ModalState::Equalizer { selected_band: 0 });
            }
            KeyCode::Char('d') => {
                self.start_lyrics_auto_fetch();
            }
            KeyCode::Char('D') => {
                self.delete_current_lyrics();
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let Some(entry) = self.playlist.selected_entry().cloned() {
                    if let Some(audio) = entry.audio_item() {
                        let path = audio.path.clone();
                        let display_name = audio.display_name.clone();
                        let (added, pos) = self.playlist.toggle_queue(path);
                        let msg = if added {
                            self.i18n.t_args("queue.added", &[
                                ("pos", &pos.to_string()),
                                ("track", &display_name),
                            ])
                        } else {
                            self.i18n.t_args("queue.removed", &[
                                ("track", &display_name),
                            ])
                        };
                        self.lyrics_toast = Some((msg, Instant::now(), false));
                    }
                }
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
            KeyCode::Char('n') | KeyCode::Char('>') => {
                self.play_next_track();
            }
            KeyCode::Char('p') | KeyCode::Char('<') => {
                self.play_prev_track();
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                self.visualizer_mode = self.visualizer_mode.next();
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.show_lyrics = !self.show_lyrics;
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

                // 1. プログレスバーのクリック -> シーク & ドラッグ開始
                if Self::is_inside_rect(col, row, self.progress_area) && self.progress_area.width > 0 {
                    let total = self.engine.total_duration_secs();
                    if total > 0.0 {
                        self.is_dragging_seekbar = true;
                        let offset = col.saturating_sub(self.progress_area.x) as f64;
                        let ratio = (offset / self.progress_area.width as f64).clamp(0.0, 1.0);
                        let target = ratio * total;
                        self.drag_target_secs = Some(target);
                        self.engine.seek(target);
                    }
                    return;
                }

                // 2. ステータス行のクリック -> 各種バッジのトグルおよび前曲/次曲ボタン
                if Self::is_inside_rect(col, row, self.status_area) {
                    let offset_x = col.saturating_sub(self.status_area.x);
                    if offset_x < 15 {
                        self.engine.toggle_pause();
                    } else if offset_x < 24 {
                        self.play_prev_track();
                    } else if offset_x < 33 {
                        self.play_next_track();
                    } else if offset_x < 46 {
                        self.playlist.toggle_repeat();
                    } else if offset_x < 59 {
                        self.playlist.toggle_shuffle();
                    } else if offset_x < 72 {
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

                // 4. プレイリスト領域のクリック -> 楽曲選択＆即時再生、またはフォルダ進入/親戻り
                if Self::is_inside_rect(col, row, self.playlist_area) {
                    let inner_y = self.playlist_area.y.saturating_add(2); // 上枠線(1) + パンくず行(1)
                    let inner_bottom = self.playlist_area.y.saturating_add(self.playlist_area.height).saturating_sub(1);
                    if row >= inner_y && row < inner_bottom {
                        let clicked_line = (row - inner_y) as usize;
                        if clicked_line < self.playlist.len() {
                            self.playlist.set_cursor(clicked_line);
                            self.cursor_moved_at = Instant::now();

                            if let Some(entry) = self.playlist.selected_entry().cloned() {
                                match entry {
                                    PlaylistEntry::ParentDir => {
                                        self.playlist.go_to_parent();
                                    }
                                    PlaylistEntry::Directory { path, .. } => {
                                        self.playlist.enter_directory(&path);
                                    }
                                    PlaylistEntry::AudioFile(_) => {
                                        self.play_current_selected();
                                    }
                                }
                            }
                        }
                    }
                    return;
                }

                // 5. その他の領域（空白領域など） -> 従来通り再生/一時停止トグル
                self.engine.toggle_pause();
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let col = mouse.column;
                if self.is_dragging_seekbar && self.progress_area.width > 0 {
                    let offset = col.saturating_sub(self.progress_area.x) as f64;
                    let ratio = (offset / self.progress_area.width as f64).clamp(0.0, 1.0);
                    let total = self.engine.total_duration_secs();
                    if total > 0.0 {
                        self.drag_target_secs = Some(ratio * total);
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.is_dragging_seekbar {
                    self.is_dragging_seekbar = false;
                    if let Some(target) = self.drag_target_secs.take() {
                        self.engine.seek(target);
                    }
                }
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
                    self.cursor_moved_at = Instant::now();
                } else {
                    self.increase_volume();
                }
            }
            MouseEventKind::ScrollDown => {
                let col = mouse.column;
                let row = mouse.row;
                if Self::is_inside_rect(col, row, self.playlist_area) {
                    self.playlist.move_cursor_down();
                    self.cursor_moved_at = Instant::now();
                } else {
                    self.decrease_volume();
                }
            }
            _ => {}
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        /// ターミナルの復元を保証する RAII ガード
        struct TerminalGuard;

        impl Drop for TerminalGuard {
            fn drop(&mut self) {
                let _ = disable_raw_mode();
                let _ = crossterm::execute!(
                    stdout(),
                    LeaveAlternateScreen,
                    crossterm::event::DisableMouseCapture,
                    crossterm::cursor::Show
                );
            }
        }

        let _guard = TerminalGuard;
        enable_raw_mode()?;
        let mut stdout_handle = stdout();
        crossterm::execute!(stdout_handle, EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout_handle);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let mut frame_count: u64 = 0;
        let target_frame_duration = Duration::from_millis(33); // 約30 FPS 上限制御
        let mut last_frame_time = Instant::now();

        while !self.should_quit {
            // 1. ノンブロッキングで到着済みイベントを一括ドレイン処理
            while event::poll(Duration::from_millis(0))? {
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

            if self.should_quit {
                break;
            }

            // 2. 次フレーム時刻までの残り時間をスリープ待機（イベント発生時は即座にウェイクアップ）
            let elapsed = last_frame_time.elapsed();
            if elapsed < target_frame_duration {
                let remaining = target_frame_duration - elapsed;
                if event::poll(remaining)? {
                    continue; // イベント到着時は先頭に戻り即時処理
                }
            }

            last_frame_time = Instant::now();
            let current_engine_state = self.engine.current_state();

            // ギャップレス再生通知の受信処理（ホットスワップ後のUI同期）
            while let Some(notif) = self.engine.poll_notification() {
                match notif {
                    EngineNotification::TrackTransitioned(path, meta) => {
                        crate::logger::info("App", &format!("Handling gapless track transition in UI: {:?}", path));
                        self.apply_gapless_track_transition(&path, meta);
                    }
                }
            }

            // 残り3秒未満での次曲バックグラウンドプリロードトリガー
            if current_engine_state == PlaybackState::Playing {
                let cur = self.engine.current_position_secs();
                let dur = self.engine.total_duration_secs();
                if dur > 3.0 && (dur - cur) <= 3.0 {
                    if let Some(next_item) = self.playlist.peek_next_track() {
                        self.engine.preload_next(&next_item.path);
                    }
                }
            }

            // 再生中から終了状態への自然遷移時のみ、次の曲へ進む（プリロードなし停止時フォールバック）
            if self.last_engine_state == PlaybackState::Playing && current_engine_state == PlaybackState::Stopped {
                self.play_next_track();
            }
            self.last_engine_state = current_engine_state;

            // リアルタイム波形データの更新
            frame_count = frame_count.wrapping_add(1);
            if frame_count % 30 == 0 {
                self.check_pending_cover_art();
            }
            self.check_folder_picker_result();
            self.check_lyrics_fetch_result();
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
                    VisualizerMode::Spectrum => {
                        // SPECTRUM: 本格FFT対数周波数解析
                        let raw_samples = self.engine.get_visualizer_raw_samples();
                        if !raw_samples.is_empty() {
                            self.spectrum_analyzer.push_samples(&raw_samples);
                        }
                        self.spectrum_analyzer.process();
                    }
                }
            } else {
                for p in &mut self.waveform_points {
                    *p *= 0.8;
                }
                self.spectrum_analyzer.process();
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
                    elapsed_ms: self.cursor_moved_at.elapsed().as_millis(),
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
                    elapsed_ms: self.track_changed_at.elapsed().as_millis(),
                };
                track_info_view.render_view(right_layout[0], f.buffer_mut());

                // 3. コントロール＆波形描画
                let cur_secs = if self.is_dragging_seekbar && self.drag_target_secs.is_some() {
                    self.drag_target_secs.unwrap()
                } else {
                    self.engine.current_position_secs()
                };

                let next_queue_title = self.playlist.peek_queue().and_then(|path| {
                    self.playlist.all_tracks().iter().find(|t| &t.path == path).map(|t| t.display_name.as_str())
                });

                let controls_view = ControlsView {
                    playback_state: &self.engine.current_state(),
                    current_position_secs: cur_secs,
                    total_duration_secs: self.engine.total_duration_secs(),
                    volume: self.engine.volume(),
                    repeat_mode: self.playlist.repeat_mode(),
                    is_shuffle: self.playlist.is_shuffle(),
                    is_focused: active_pane == UiPane::Controls,
                    visualizer_mode: self.visualizer_mode,
                    waveform_points: &self.waveform_points,
                    spectrum_bands: self.spectrum_analyzer.get_bands(),
                    lyrics: self.current_lyrics.as_ref(),
                    show_lyrics: self.show_lyrics,
                    is_fetching_lyrics: self.lyrics_fetch_rx.is_some(),
                    lyrics_toast: self.lyrics_toast.as_ref(),
                    next_queue_track: next_queue_title,
                    i18n: &self.i18n,
                    theme: &self.theme,
                };
                f.render_widget(controls_view, right_layout[1]);

                // 4. 最下部キーバインドバー描画
                let footer_view = FooterView {
                    i18n: &self.i18n,
                    theme: &self.theme,
                    is_searching: self.is_searching,
                    search_query: &self.search_query,
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
                    ModalState::Equalizer { selected_band } => {
                        let m = EqualizerModal {
                            enabled: self.eq_enabled,
                            gains: &self.eq_gains,
                            selected_band: *selected_band,
                            current_preset: &self.eq_preset,
                            i18n: &self.i18n,
                            theme: &self.theme,
                        };
                        f.render_widget(m, size);
                    }
                    ModalState::None => {}
                }
            })?;
        }

        // セッション・設定の自動保存
        if let Err(e) = self.save_session() {
            crate::logger::error("App", &format!("Failed to save session config: {}", e));
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
        let mut config = AppConfig::default();
        config.audio.output_mode = "Mock".to_string();
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

            // Alt+Up / Alt+Down で並び替えディスパッチ（空プレイリストでもクラッシュしない）
            app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
            app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));

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

            // 5. Shift+Left / Shift+Right による曲スキップテスト
            app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
            app.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
            app.handle_key_event(KeyEvent::new(KeyCode::Char('>'), KeyModifiers::NONE));
            // 6. / キーによる検索モード遷移・入力・解除テスト
            app.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            assert!(app.is_searching);
            app.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
            app.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
            app.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
            app.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
            assert_eq!(app.search_query, "test");
            app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
            assert_eq!(app.search_query, "tes");
            app.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(!app.is_searching);
            assert_eq!(app.search_query, "");

            // qキーで終了
            app.handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
            assert!(app.should_quit);
        }
    }

    #[test]
    fn test_app_mouse_controls() {
        let mut config = AppConfig::default();
        config.audio.output_mode = "Mock".to_string();
        if let Ok(mut app) = App::new(&config, None) {
            // 領域の設定
            app.playlist_area = Rect::new(0, 0, 30, 20);
            app.progress_area = Rect::new(32, 12, 80, 1);
            app.status_area = Rect::new(32, 13, 80, 1);
            app.track_info_area = Rect::new(32, 0, 80, 10);
            app.controls_area = Rect::new(32, 11, 80, 9);
            app.engine.set_total_duration_for_test(200.0);

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

            // 2. プログレスバーのクリック＆ドラッグによるシーク操作テスト
            assert!(!app.is_dragging_seekbar);
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 42,
                row: 12,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.is_dragging_seekbar);
            assert!(app.drag_target_secs.is_some());

            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 52,
                row: 12,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.is_dragging_seekbar);
            assert!(app.drag_target_secs.is_some());

            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 52,
                row: 12,
                modifiers: KeyModifiers::NONE,
            });
            assert!(!app.is_dragging_seekbar);
            assert!(app.drag_target_secs.is_none());

            // 3. ステータス行クリックによる前曲/次曲ボタテスト (offset 18: |◀, offset 27: ▶|)
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 18,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 27,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });

            // 4. ステータス行クリックによるリピートトグルテスト (offset 38: LOOPバッジ)
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::Off);
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 38,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(app.playlist.repeat_mode(), crate::playlist::manager::RepeatMode::All);

            // 5. ステータス行クリックによるシャッフルトグルテスト (offset 50: SHUFバッジ)
            assert!(!app.playlist.is_shuffle());
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 32 + 50,
                row: 13,
                modifiers: KeyModifiers::NONE,
            });
            assert!(app.playlist.is_shuffle());

            // 6. トラック情報領域クリックによる出力モードトグルテスト
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

    #[test]
    fn test_app_session_save_and_restore() {
        let mut config = AppConfig::default();
        config.audio.output_mode = "Mock".to_string();
        config.ui.show_lyrics = true;
        config.session.last_position_secs = 12.5;

        if let Ok(mut app) = App::new(&config, None) {
            // 初期状態がconfigから復元されているか検証
            assert!(app.show_lyrics);

            // 状態を変更して保存
            app.show_lyrics = false;
            app.engine.set_volume(0.60);

            let _ = app.save_session();
            assert!(!app.config.ui.show_lyrics);
            assert_eq!((app.config.audio.volume * 100.0).round() as u32, 60);
        }
    }
}
