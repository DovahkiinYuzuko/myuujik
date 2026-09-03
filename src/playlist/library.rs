use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FavoriteTrack {
    pub path: PathBuf,
    pub display_name: String,
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryItem {
    pub path: PathBuf,
    pub display_name: String,
    pub played_at: u64,
    #[serde(default = "default_play_count")]
    pub play_count: u32,
}

fn default_play_count() -> u32 {
    1
}

#[derive(Debug, Clone)]
pub struct LibraryManager {
    favorites: Vec<FavoriteTrack>,
    history: Vec<HistoryItem>,
    favorites_path: PathBuf,
    history_path: PathBuf,
    max_history_items: usize,
}

impl LibraryManager {
    pub fn new() -> Self {
        let base_dir = Self::determine_base_dir();
        let favorites_path = base_dir.join("favorites.json");
        let history_path = base_dir.join("history.json");

        let favorites = Self::load_json::<Vec<FavoriteTrack>>(&favorites_path).unwrap_or_default();
        let history = Self::load_json::<Vec<HistoryItem>>(&history_path).unwrap_or_default();

        Self {
            favorites,
            history,
            favorites_path,
            history_path,
            max_history_items: 100,
        }
    }

    pub fn with_paths(favorites_path: PathBuf, history_path: PathBuf) -> Self {
        let favorites = Self::load_json::<Vec<FavoriteTrack>>(&favorites_path).unwrap_or_default();
        let history = Self::load_json::<Vec<HistoryItem>>(&history_path).unwrap_or_default();

        Self {
            favorites,
            history,
            favorites_path,
            history_path,
            max_history_items: 100,
        }
    }

    fn determine_base_dir() -> PathBuf {
        let local_cfg = PathBuf::from("config.toml");
        if local_cfg.exists() {
            return PathBuf::from(".");
        }
        if let Some(proj_dirs) = directories::ProjectDirs::from("com", "YuzukoUnderson", "myuujik") {
            return proj_dirs.config_dir().to_path_buf();
        }
        PathBuf::from(".")
    }

    fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(data) = serde_json::from_str::<T>(&content) {
                    return Some(data);
                }
            }
        }
        None
    }

    fn save_json<T: Serialize>(path: &Path, data: &T) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = fs::create_dir_all(parent);
            }
        }
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, content)
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    // --- Favorites ---

    pub fn favorites(&self) -> &[FavoriteTrack] {
        &self.favorites
    }

    pub fn is_favorite<P: AsRef<Path>>(&self, path: P) -> bool {
        let p = path.as_ref();
        self.favorites.iter().any(|f| f.path == p)
    }

    pub fn toggle_favorite<P: AsRef<Path>>(&mut self, path: P, display_name: String) -> bool {
        let p = path.as_ref();
        if let Some(pos) = self.favorites.iter().position(|f| f.path == p) {
            self.favorites.remove(pos);
            let _ = Self::save_json(&self.favorites_path, &self.favorites);
            false
        } else {
            self.favorites.push(FavoriteTrack {
                path: p.to_path_buf(),
                display_name,
                added_at: Self::current_timestamp(),
            });
            let _ = Self::save_json(&self.favorites_path, &self.favorites);
            true
        }
    }

    pub fn remove_favorite(&mut self, index: usize) -> Option<FavoriteTrack> {
        if index < self.favorites.len() {
            let removed = self.favorites.remove(index);
            let _ = Self::save_json(&self.favorites_path, &self.favorites);
            Some(removed)
        } else {
            None
        }
    }

    // --- History ---

    pub fn history(&self) -> &[HistoryItem] {
        &self.history
    }

    pub fn record_playback<P: AsRef<Path>>(&mut self, path: P, display_name: String) {
        let p = path.as_ref();
        let now = Self::current_timestamp();

        if let Some(pos) = self.history.iter().position(|h| h.path == p) {
            let mut item = self.history.remove(pos);
            item.played_at = now;
            item.play_count = item.play_count.saturating_add(1);
            item.display_name = display_name;
            self.history.insert(0, item);
        } else {
            let item = HistoryItem {
                path: p.to_path_buf(),
                display_name,
                played_at: now,
                play_count: 1,
            };
            self.history.insert(0, item);
        }

        if self.history.len() > self.max_history_items {
            self.history.truncate(self.max_history_items);
        }

        let _ = Self::save_json(&self.history_path, &self.history);
    }

    pub fn remove_history(&mut self, index: usize) -> Option<HistoryItem> {
        if index < self.history.len() {
            let removed = self.history.remove(index);
            let _ = Self::save_json(&self.history_path, &self.history);
            Some(removed)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_favorites_toggle_and_query() {
        let temp_dir = std::env::temp_dir().join(format!("myuujik_test_fav_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let fav_file = temp_dir.join("favorites.json");
        let hist_file = temp_dir.join("history.json");

        let mut lib = LibraryManager::with_paths(fav_file.clone(), hist_file.clone());
        let track_p = PathBuf::from("test/track1.flac");

        assert_eq!(lib.is_favorite(&track_p), false);

        // トグルで追加
        let added = lib.toggle_favorite(&track_p, "Track One".to_string());
        assert_eq!(added, true);
        assert_eq!(lib.is_favorite(&track_p), true);
        assert_eq!(lib.favorites().len(), 1);

        // 再度ロードして永続化確認
        let lib2 = LibraryManager::with_paths(fav_file.clone(), hist_file.clone());
        assert_eq!(lib2.is_favorite(&track_p), true);

        // トグルで解除
        let removed = lib.toggle_favorite(&track_p, "Track One".to_string());
        assert_eq!(removed, false);
        assert_eq!(lib.is_favorite(&track_p), false);
        assert_eq!(lib.favorites().is_empty(), true);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_history_record_dedup_and_cap() {
        let temp_dir = std::env::temp_dir().join(format!("myuujik_test_hist_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let fav_file = temp_dir.join("favorites.json");
        let hist_file = temp_dir.join("history.json");

        let mut lib = LibraryManager::with_paths(fav_file, hist_file);
        lib.max_history_items = 3;

        lib.record_playback(PathBuf::from("a.mp3"), "Track A".to_string());
        lib.record_playback(PathBuf::from("b.mp3"), "Track B".to_string());
        assert_eq!(lib.history().len(), 2);
        assert_eq!(lib.history()[0].display_name, "Track B");

        // 重複再生 -> 最新に移動 & 回数インクリメント
        lib.record_playback(PathBuf::from("a.mp3"), "Track A".to_string());
        assert_eq!(lib.history().len(), 2);
        assert_eq!(lib.history()[0].display_name, "Track A");
        assert_eq!(lib.history()[0].play_count, 2);

        // キャパシティ上限テスト
        lib.record_playback(PathBuf::from("c.mp3"), "Track C".to_string());
        lib.record_playback(PathBuf::from("d.mp3"), "Track D".to_string());
        assert_eq!(lib.history().len(), 3);
        assert_eq!(lib.history()[0].display_name, "Track D");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
