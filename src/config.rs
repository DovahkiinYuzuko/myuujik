use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub playback: PlaybackConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub equalizer: EqualizerConfig,
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioConfig {
    #[serde(default = "default_output_mode")]
    pub output_mode: String, // "Shared" or "Exclusive"
    #[serde(default = "default_output_device")]
    pub output_device: String,
    #[serde(default = "default_volume")]
    pub volume: f32, // 0.0 .. 1.0
    #[serde(default)]
    pub bit_perfect_lock: bool,
}

fn default_output_mode() -> String {
    "Shared".to_string()
}
fn default_output_device() -> String {
    "Default".to_string()
}
fn default_volume() -> f32 {
    0.85
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            output_mode: default_output_mode(),
            output_device: default_output_device(),
            volume: default_volume(),
            bit_perfect_lock: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybackConfig {
    #[serde(default = "default_repeat_mode")]
    pub repeat_mode: String, // "None", "All", "Single"
    #[serde(default = "default_shuffle")]
    pub shuffle: bool,
    #[serde(default = "default_true")]
    pub normalize_loudness: bool,
    #[serde(default = "default_crossfade_secs")]
    pub crossfade_secs: f32,
}

fn default_repeat_mode() -> String {
    "None".to_string()
}
fn default_shuffle() -> bool {
    false
}
fn default_crossfade_secs() -> f32 {
    0.0
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            repeat_mode: default_repeat_mode(),
            shuffle: default_shuffle(),
            normalize_loudness: default_true(),
            crossfade_secs: default_crossfade_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiConfig {
    #[serde(default = "default_image_protocol")]
    pub image_protocol: String, // "Auto", "Sixel", "Kitty", "Iterm2", "HalfBlock"
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_visualizer_mode")]
    pub visualizer_mode: String, // "Type4" (Wave) or "Type3" (Meter)
    #[serde(default)]
    pub show_lyrics: bool,
}

fn default_image_protocol() -> String {
    "Auto".to_string()
}
fn default_theme() -> String {
    "catppuccin-mocha".to_string()
}
fn default_locale() -> String {
    "ja".to_string()
}
fn default_visualizer_mode() -> String {
    "Type4".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            image_protocol: default_image_protocol(),
            theme: default_theme(),
            locale: default_locale(),
            visualizer_mode: default_visualizer_mode(),
            show_lyrics: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionConfig {
    pub last_opened_path: Option<String>,
    #[serde(default)]
    pub last_track_index: usize,
    pub last_track_path: Option<String>,
    #[serde(default)]
    pub last_position_secs: f64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            audio: AudioConfig::default(),
            playback: PlaybackConfig::default(),
            ui: UiConfig::default(),
            equalizer: EqualizerConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EqualizerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_eq_gains")]
    pub gains: Vec<f32>,
    #[serde(default = "default_eq_preset")]
    pub preset: String,
}

fn default_true() -> bool {
    true
}

fn default_eq_gains() -> Vec<f32> {
    vec![0.0; 10]
}

fn default_eq_preset() -> String {
    "Flat".to_string()
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            gains: default_eq_gains(),
            preset: default_eq_preset(),
        }
    }
}

impl AppConfig {
    /// 設定ファイルの標準保存パスを特定（カレントディレクトリ優先、なければ %APPDATA%/myuujik/config.toml）
    pub fn get_config_path() -> PathBuf {
        let local_path = PathBuf::from("config.toml");
        if local_path.exists() {
            return local_path;
        }

        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "YuzukoUnderson", "myuujik") {
            let config_dir = proj_dirs.config_dir();
            return config_dir.join("config.toml");
        }

        local_path
    }

    /// 設定ファイルを読み込み（存在しない場合はデフォルト値を生成して保存）
    pub fn load() -> Self {
        let config_path = Self::get_config_path();
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(config) = toml::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }

        let default_cfg = Self::default();
        let _ = default_cfg.save_to(&config_path);
        default_cfg
    }

    /// 指定パスへ設定を保存
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let serialized = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        fs::write(path, serialized)
    }

    /// 標準パスへ現在の設定を保存
    pub fn save(&self) -> std::io::Result<()> {
        let config_path = Self::get_config_path();
        self.save_to(&config_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_serialization() {
        let cfg = AppConfig::default();
        let toml_str = toml::to_string_pretty(&cfg).expect("failed to serialize toml");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("failed to deserialize toml");
        assert_eq!(cfg, parsed);
        assert_eq!(parsed.audio.volume, 0.85);
        assert_eq!(parsed.ui.locale, "ja");
        assert_eq!(parsed.ui.visualizer_mode, "Type4");
        assert_eq!(parsed.ui.show_lyrics, false);
        assert_eq!(parsed.session.last_track_path, None);
        assert_eq!(parsed.session.last_position_secs, 0.0);
    }

    #[test]
    fn test_custom_config_session_persistence() {
        let mut cfg = AppConfig::default();
        cfg.ui.show_lyrics = true;
        cfg.session.last_opened_path = Some("C:/Music".to_string());
        cfg.session.last_track_index = 3;
        cfg.session.last_track_path = Some("C:/Music/test.flac".to_string());
        cfg.session.last_position_secs = 124.5;
        cfg.equalizer.enabled = true;
        cfg.equalizer.preset = "Rock".to_string();
        cfg.equalizer.gains = vec![5.0, 3.5, 2.0, -0.5, -1.5, -0.5, 1.5, 3.0, 4.0, 4.5];
        cfg.playback.normalize_loudness = true;
        cfg.playback.crossfade_secs = 3.0;

        let toml_str = toml::to_string_pretty(&cfg).expect("failed to serialize toml");
        let parsed: AppConfig = toml::from_str(&toml_str).expect("failed to deserialize toml");
        assert_eq!(parsed.ui.show_lyrics, true);
        assert_eq!(parsed.session.last_opened_path.as_deref(), Some("C:/Music"));
        assert_eq!(parsed.session.last_track_index, 3);
        assert_eq!(parsed.session.last_track_path.as_deref(), Some("C:/Music/test.flac"));
        assert_eq!(parsed.session.last_position_secs, 124.5);
        assert_eq!(parsed.equalizer.enabled, true);
        assert_eq!(parsed.equalizer.preset, "Rock");
        assert_eq!(parsed.equalizer.gains.len(), 10);
        assert_eq!(parsed.equalizer.gains[0], 5.0);
        assert_eq!(parsed.playback.normalize_loudness, true);
        assert_eq!(parsed.playback.crossfade_secs, 3.0);
    }
}

