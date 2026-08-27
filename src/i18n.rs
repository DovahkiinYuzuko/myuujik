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
}
