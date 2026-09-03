use rokeeru_core::RokeeruLoader;
use serde_json::Value;
use std::path::PathBuf;

pub struct I18n {
    loader: Option<RokeeruLoader>,
    fallback_ja: Value,
    fallback_en: Value,
    current_locale: String,
}

const EMBEDDED_JA: &str = include_str!("../locales/ja.json");
const EMBEDDED_EN: &str = include_str!("../locales/en.json");

impl I18n {
    /// ロケールディレクトリからI18nを初期化（ディレクトリが存在しない場合は内蔵JSONをフォールバックとして使用）
    pub fn new(locales_dir: PathBuf, default_locale: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let fallback_ja: Value = serde_json::from_str(EMBEDDED_JA)?;
        let fallback_en: Value = serde_json::from_str(EMBEDDED_EN)?;

        let loader = if locales_dir.exists() && locales_dir.is_dir() {
            if let Ok(l) = RokeeruLoader::new(&locales_dir, default_locale) {
                // 事前ロード
                let _ = l.load(default_locale);
                Some(l)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            loader,
            fallback_ja,
            fallback_en,
            current_locale: default_locale.to_string(),
        })
    }

    /// 現在のロケールを変更
    pub fn set_locale(&mut self, locale: &str) {
        self.current_locale = locale.to_string();
        if let Some(loader) = &self.loader {
            let _ = loader.load(locale);
        }
    }

    /// 利用可能なロケール一覧（言語コード）を取得（rokeeru-core の自動走査結果を活用）
    pub fn available_locales(&self) -> Vec<String> {
        if let Some(loader) = &self.loader {
            let mut langs = loader.languages();
            langs.sort();
            if !langs.is_empty() {
                return langs;
            }
        }
        vec!["en".to_string(), "ja".to_string()]
    }

    /// 次のロケールへ順繰りに切り替え、新しいロケール識別子を返す
    pub fn switch_to_next_locale(&mut self) -> String {
        let locales = self.available_locales();
        if locales.is_empty() {
            return self.current_locale.clone();
        }
        let current_idx = locales.iter().position(|l| l == &self.current_locale).unwrap_or(0);
        let next_idx = (current_idx + 1) % locales.len();
        let next_locale = locales[next_idx].clone();
        self.set_locale(&next_locale);
        next_locale
    }

    /// 現在のロケール識別子を取得
    pub fn current_locale(&self) -> &str {
        &self.current_locale
    }

    /// 現在の言語表示名を取得（例: "日本語", "English"）
    pub fn language_name(&self) -> String {
        self.t("language_name")
    }

    /// ドット記法キー（例: "app.title"）で翻訳文字列を取得
    pub fn t(&self, key: &str) -> String {
        // 1. rokeeru_core の RokeeruLoader から取得を試みる
        if let Some(loader) = &self.loader {
            if let Ok(json) = loader.load(&self.current_locale) {
                if let Some(val) = Self::lookup_key(&json, key) {
                    return val;
                }
            }
        }

        // 2. 現在のロケールに応じた内蔵フォールバックJSONを探索
        let target_fallback = if self.current_locale.starts_with("ja") {
            &self.fallback_ja
        } else {
            &self.fallback_en
        };

        if let Some(val) = Self::lookup_key(target_fallback, key) {
            return val;
        }

        // 3. 英語フォールバックJSONを探索
        if let Some(val) = Self::lookup_key(&self.fallback_en, key) {
            return val;
        }

        format!("<missing: {}>", key)
    }

    /// プレースホルダー置換付きテキスト取得（例: `t_args("playlist.total_tracks", &[("count", "12")])`）
    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut template = self.t(key);
        for (placeholder, val) in args {
            let target = format!("{{{}}}", placeholder);
            template = template.replace(&target, val);
        }
        template
    }

    fn lookup_key(root: &Value, key: &str) -> Option<String> {
        let mut current = root;
        for part in key.split('.') {
            if let Some(next) = current.get(part) {
                current = next;
            } else {
                return None;
            }
        }
        current.as_str().map(|s| s.to_string())
    }
}

/// OS のロケール設定を自動判定（日本語環境であれば "ja"、それ以外はデフォルト "en" を返却）
pub fn detect_os_locale() -> String {
    // 1. 環境変数チェック (Linux, macOS, Git Bash, WSL, Windows Terminal 等)
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.starts_with("ja") {
                return "ja".to_string();
            }
        }
    }

    // 2. Windows 専用の判定 (Kernel32 GetUserDefaultUILanguage)
    #[cfg(windows)]
    {
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // 0x0411 は日本語 (1041)
        if unsafe { GetUserDefaultUILanguage() } == 0x0411 {
            return "ja".to_string();
        }
    }

    // デフォルトは英語
    "en".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i18n_keys_and_fallbacks() {
        let mut i18n = I18n::new(PathBuf::from("locales"), "ja").expect("failed to init i18n");
        assert_eq!(i18n.language_name(), "日本語");
        assert_eq!(i18n.t("app.title"), "myuujik");

        let formatted = i18n.t_args("playlist.total_tracks", &[("count", "42")]);
        assert_eq!(formatted, "全 42 曲");

        i18n.set_locale("en");
        assert_eq!(i18n.language_name(), "English");
        let formatted_en = i18n.t_args("playlist.total_tracks", &[("count", "42")]);
        assert_eq!(formatted_en, "Total 42 tracks");
    }

    #[test]
    fn test_all_ui_keys_exist_in_both_locales() {
        let keys = [
            "app.title", "app.subtitle", "app.mode_shared", "app.mode_exclusive",
            "playlist.header", "playlist.empty", "playlist.no_directory", "playlist.parent_dir",
            "track_info.header", "track_info.title", "track_info.artist", "track_info.album",
            "track_info.format", "track_info.output_mode", "track_info.unknown_artist",
            "track_info.unknown_album", "track_info.unknown_track", "track_info.no_track_loaded",
            "track_info.no_album_art_line1", "track_info.no_album_art_line2",
            "track_info.badge_exclusive", "track_info.badge_shared_fallback", "track_info.badge_shared",
            "controls.header", "controls.volume", "controls.vol_label", "controls.progress",
            "controls.status_playing", "controls.status_paused", "controls.status_stopped",
            "controls.loop_off", "controls.loop_all", "controls.loop_single",
            "controls.shuf_on", "controls.shuf_off",
            "modal.device_select", "modal.help", "modal.error",
            "modal.press_esc_q", "modal.press_dismiss", "modal.default_badge", "modal.key_reference",
            "shortcuts.play_pause", "shortcuts.select_track", "shortcuts.play_selected",
            "shortcuts.seek", "shortcuts.volume", "shortcuts.repeat", "shortcuts.shuffle",
            "shortcuts.exclusive", "shortcuts.devices", "shortcuts.pane_switch",
            "shortcuts.next_prev_track", "shortcuts.help", "shortcuts.quit",
        ];

        for locale in &["ja", "en"] {
            let mut i18n = I18n::new(PathBuf::from("locales"), locale).expect("failed to init i18n");
            i18n.set_locale(locale);
            for key in &keys {
                let val = i18n.t(key);
                assert!(
                    !val.starts_with("<missing:"),
                    "Missing translation key '{}' in locale '{}'",
                    key,
                    locale
                );
            }
        }
    }

    #[test]
    fn test_locale_discovery_and_switching() {
        let mut i18n = I18n::new(PathBuf::from("locales"), "en").expect("failed to init i18n");
        let locales = i18n.available_locales();
        assert!(locales.contains(&"en".to_string()));
        assert!(locales.contains(&"ja".to_string()));

        assert_eq!(i18n.current_locale(), "en");
        let next = i18n.switch_to_next_locale();
        assert_eq!(next, "ja");
        assert_eq!(i18n.current_locale(), "ja");
        assert_eq!(i18n.language_name(), "日本語");

        let back_to_en = i18n.switch_to_next_locale();
        assert_eq!(back_to_en, "en");
        assert_eq!(i18n.current_locale(), "en");
        assert_eq!(i18n.language_name(), "English");

        let os_locale = detect_os_locale();
        assert!(os_locale == "ja" || os_locale == "en");
    }
}
