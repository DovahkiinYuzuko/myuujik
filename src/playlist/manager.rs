use crate::playlist::item::{PlaylistEntry, PlaylistItem};
use crate::playlist::scanner::AudioScanner;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    Single,
}

#[derive(Debug, Clone)]
pub struct PlaylistManager {
    root_path: Option<PathBuf>,
    current_dir: Option<PathBuf>,
    entries: Vec<PlaylistEntry>,
    items: Vec<PlaylistItem>,
    all_tracks: Vec<PlaylistItem>,
    cursor: usize,
    current_playing_index: Option<usize>,
    current_playing_path: Option<PathBuf>,
    repeat_mode: RepeatMode,
    shuffle_enabled: bool,
    shuffle_indices: Vec<usize>,
    shuffle_pos: usize,
}

impl PlaylistManager {
    pub fn new() -> Self {
        Self {
            root_path: None,
            current_dir: None,
            entries: Vec::new(),
            items: Vec::new(),
            all_tracks: Vec::new(),
            cursor: 0,
            current_playing_index: None,
            current_playing_path: None,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            shuffle_indices: Vec::new(),
            shuffle_pos: 0,
        }
    }

    pub fn load_path<P: AsRef<Path>>(&mut self, path: P) -> usize {
        let path_ref = path.as_ref();
        if !path_ref.exists() {
            return 0;
        }

        let abs_path = std::fs::canonicalize(path_ref).unwrap_or_else(|_| path_ref.to_path_buf());

        let scan_target = if abs_path.is_file() {
            let parent = abs_path.parent().unwrap_or(&abs_path).to_path_buf();
            self.root_path = Some(parent.clone());
            self.current_dir = Some(parent.clone());
            parent
        } else {
            self.root_path = Some(abs_path.clone());
            self.current_dir = Some(abs_path.clone());
            abs_path
        };

        let raw_paths = AudioScanner::scan_path(&scan_target);
        self.all_tracks = raw_paths
            .into_iter()
            .enumerate()
            .map(|(idx, p)| PlaylistItem::from_path(idx, p))
            .collect();

        self.cursor = 0;
        self.current_playing_index = None;
        self.current_playing_path = None;
        self.rebuild_shuffle_indices();
        self.refresh_entries();
        self.all_tracks.len()
    }

    pub fn refresh_entries(&mut self) {
        self.entries.clear();
        self.items.clear();

        if let Some(ref current) = self.current_dir {
            // 親ディレクトリへの復帰リンク（ルートより深い場合のみ）
            if let Some(ref root) = self.root_path {
                if current != root && current.starts_with(root) {
                    self.entries.push(PlaylistEntry::ParentDir);
                }
            }

            let (subdirs, audio_files) = AudioScanner::scan_directory_shallow(current);

            for d in subdirs {
                let name = d.file_name().and_then(|s| s.to_str()).unwrap_or("folder").to_string();
                self.entries.push(PlaylistEntry::Directory {
                    name: format!("{}/", name),
                    path: d,
                });
            }

            for (idx, f) in audio_files.into_iter().enumerate() {
                let item = PlaylistItem::from_path(idx, f);
                self.items.push(item.clone());
                self.entries.push(PlaylistEntry::AudioFile(item));
            }
        }

        self.sync_current_index_with_items();

        if !self.entries.is_empty() {
            if self.cursor >= self.entries.len() {
                self.cursor = self.entries.len() - 1;
            }
        } else {
            self.cursor = 0;
        }
    }

    fn sync_current_index_with_items(&mut self) {
        if let Some(ref path) = self.current_playing_path {
            self.current_playing_index = self.items.iter().position(|it| &it.path == path);
        } else {
            self.current_playing_index = None;
        }
    }

    pub fn enter_directory(&mut self, path: &Path) -> bool {
        if path.is_dir() {
            let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            self.current_dir = Some(canonical);
            self.cursor = 0;
            self.refresh_entries();
            true
        } else {
            false
        }
    }

    pub fn go_to_parent(&mut self) -> bool {
        if let (Some(ref current), Some(ref root)) = (&self.current_dir, &self.root_path) {
            if current == root || !current.starts_with(root) {
                return false; // ルート上限境界ガード！
            }
            if let Some(parent) = current.parent() {
                if parent.starts_with(root) {
                    self.current_dir = Some(parent.to_path_buf());
                    self.cursor = 0;
                    self.refresh_entries();
                    return true;
                }
            }
        }
        false
    }

    pub fn breadcrumb(&self) -> String {
        if let (Some(ref current), Some(ref root)) = (&self.current_dir, &self.root_path) {
            let root_name = root.file_name().and_then(|s| s.to_str()).unwrap_or("root");
            if current == root {
                format!("> {}", root_name)
            } else if let Ok(rel) = current.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', " / ");
                format!("> {} / {}", root_name, rel_str)
            } else {
                format!("> {}", current.file_name().and_then(|s| s.to_str()).unwrap_or("folder"))
            }
        } else {
            "> [No directory]".to_string()
        }
    }

    pub fn entries(&self) -> &[PlaylistEntry] {
        &self.entries
    }

    pub fn selected_entry(&self) -> Option<&PlaylistEntry> {
        self.entries.get(self.cursor)
    }

    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn move_cursor_up(&mut self) {
        if self.entries.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = self.entries.len() - 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.entries.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
    }

    pub fn set_cursor(&mut self, index: usize) {
        if !self.entries.is_empty() {
            self.cursor = index.min(self.entries.len() - 1);
        }
    }

    pub fn selected_item(&self) -> Option<&PlaylistItem> {
        self.selected_entry().and_then(|entry| entry.audio_item())
    }

    pub fn current_track(&self) -> Option<&PlaylistItem> {
        if let Some(ref path) = self.current_playing_path {
            self.all_tracks.iter().find(|t| &t.path == path)
        } else {
            None
        }
    }

    pub fn current_track_path(&self) -> Option<&PathBuf> {
        self.current_playing_path.as_ref()
    }

    pub fn all_tracks(&self) -> &[PlaylistItem] {
        &self.all_tracks
    }

    pub fn current_playing_index(&self) -> Option<usize> {
        self.current_playing_index
    }

    pub fn select_and_play_path(&mut self, path: &Path) -> Option<&PlaylistItem> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.current_playing_path = Some(canonical.clone());
        self.sync_current_index_with_items();

        // カレントエントリ内に存在すればカーソルを合わせる
        if let Some(entry_idx) = self.entries.iter().position(|e| match e {
            PlaylistEntry::AudioFile(it) => it.path == canonical,
            _ => false,
        }) {
            self.cursor = entry_idx;
        }

        // シャッフル先頭を現在曲にしてリセット
        if self.shuffle_enabled {
            if let Some(track_idx) = self.all_tracks.iter().position(|t| t.path == canonical) {
                if let Some(pos) = self.shuffle_indices.iter().position(|&idx| idx == track_idx) {
                    self.shuffle_indices.swap(0, pos);
                    self.shuffle_pos = 0;
                }
            }
        }

        self.all_tracks.iter().find(|t| t.path == canonical)
    }

    pub fn select_and_play(&mut self, index: usize) -> Option<&PlaylistItem> {
        if index < self.items.len() {
            let path = self.items[index].path.clone();
            self.select_and_play_path(&path)
        } else {
            None
        }
    }

    pub fn select_and_play_entry(&mut self, entry_idx: usize) -> Option<&PlaylistItem> {
        if entry_idx < self.entries.len() {
            self.cursor = entry_idx;
            if let Some(audio) = self.entries[entry_idx].audio_item() {
                let path = audio.path.clone();
                return self.select_and_play_path(&path);
            }
        }
        None
    }

    pub fn next_track(&mut self) -> Option<&PlaylistItem> {
        if self.all_tracks.is_empty() {
            return None;
        }

        if self.repeat_mode == RepeatMode::Single {
            if let Some(ref path) = self.current_playing_path {
                return self.all_tracks.iter().find(|t| &t.path == path);
            }
        }

        if self.shuffle_enabled && !self.shuffle_indices.is_empty() {
            if self.shuffle_pos + 1 < self.shuffle_indices.len() {
                self.shuffle_pos += 1;
                let next_idx = self.shuffle_indices[self.shuffle_pos];
                let path = self.all_tracks[next_idx].path.clone();
                return self.select_and_play_path(&path);
            } else if self.repeat_mode == RepeatMode::All {
                self.reshuffle();
                let next_idx = self.shuffle_indices[0];
                let path = self.all_tracks[next_idx].path.clone();
                return self.select_and_play_path(&path);
            } else {
                self.current_playing_path = None;
                self.current_playing_index = None;
                return None;
            }
        }

        // 通常順再生（カーナビ式：フォルダ跨ぎ連続再生）
        let current_pos = self.current_playing_path.as_ref().and_then(|p| {
            self.all_tracks.iter().position(|t| &t.path == p)
        });

        let next_idx = match current_pos {
            Some(curr) => {
                if curr + 1 < self.all_tracks.len() {
                    curr + 1
                } else if self.repeat_mode == RepeatMode::All {
                    0
                } else {
                    // RepeatMode::Off: 全曲の終端で完全停止！
                    self.current_playing_path = None;
                    self.current_playing_index = None;
                    return None;
                }
            }
            None => 0,
        };

        let path = self.all_tracks[next_idx].path.clone();
        self.select_and_play_path(&path)
    }

    pub fn prev_track(&mut self) -> Option<&PlaylistItem> {
        if self.all_tracks.is_empty() {
            return None;
        }

        if self.shuffle_enabled && !self.shuffle_indices.is_empty() {
            if self.shuffle_pos > 0 {
                self.shuffle_pos -= 1;
                let prev_idx = self.shuffle_indices[self.shuffle_pos];
                let path = self.all_tracks[prev_idx].path.clone();
                return self.select_and_play_path(&path);
            } else if self.repeat_mode == RepeatMode::All {
                self.shuffle_pos = self.shuffle_indices.len() - 1;
                let prev_idx = self.shuffle_indices[self.shuffle_pos];
                let path = self.all_tracks[prev_idx].path.clone();
                return self.select_and_play_path(&path);
            } else {
                return self.current_track();
            }
        }

        // 通常順前曲
        let current_pos = self.current_playing_path.as_ref().and_then(|p| {
            self.all_tracks.iter().position(|t| &t.path == p)
        });

        let prev_idx = match current_pos {
            Some(curr) => {
                if curr > 0 {
                    curr - 1
                } else if self.repeat_mode == RepeatMode::All {
                    self.all_tracks.len() - 1
                } else {
                    0
                }
            }
            None => 0,
        };

        let path = self.all_tracks[prev_idx].path.clone();
        self.select_and_play_path(&path)
    }

    pub fn toggle_repeat(&mut self) -> RepeatMode {
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::Single,
            RepeatMode::Single => RepeatMode::Off,
        };
        self.repeat_mode
    }

    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    pub fn toggle_shuffle(&mut self) -> bool {
        self.shuffle_enabled = !self.shuffle_enabled;
        if self.shuffle_enabled {
            self.reshuffle();
        }
        self.shuffle_enabled
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle_enabled = shuffle;
        if self.shuffle_enabled {
            self.reshuffle();
        }
    }

    pub fn is_shuffle(&self) -> bool {
        self.shuffle_enabled
    }

    fn rebuild_shuffle_indices(&mut self) {
        self.shuffle_indices = (0..self.all_tracks.len()).collect();
        if self.shuffle_enabled {
            self.reshuffle();
        }
    }

    fn reshuffle(&mut self) {
        let mut rng = thread_rng();
        self.shuffle_indices.shuffle(&mut rng);
        self.shuffle_pos = 0;
    }
}

impl Default for PlaylistManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_manager_navigation_and_modes() {
        let mut pm = PlaylistManager::new();
        assert!(pm.is_empty());

        let count = pm.load_path("sample");
        if count >= 3 {
            assert!(pm.len() > 0);
            assert_eq!(pm.cursor(), 0);

            // カーソル移動巡回
            pm.move_cursor_down();
            assert_eq!(pm.cursor(), 1);
            pm.move_cursor_up();
            assert_eq!(pm.cursor(), 0);
            pm.move_cursor_up();
            assert_eq!(pm.cursor(), pm.len() - 1); // ループ

            // 再生開始
            let track0_path = pm.select_and_play(0).unwrap().path.clone();
            assert_eq!(pm.current_track_path(), Some(&track0_path));

            // RepeatMode::Off の動作（全曲終端でNone完全停止）
            pm.set_repeat_mode(RepeatMode::Off);
            let last_path = pm.all_tracks()[count - 1].path.clone();
            pm.select_and_play_path(&last_path);
            assert!(pm.next_track().is_none());
            assert_eq!(pm.current_track_path(), None);

            // RepeatMode::All の動作（終端で先頭へループ）
            pm.set_repeat_mode(RepeatMode::All);
            pm.select_and_play_path(&last_path);
            let looped = pm.next_track().unwrap();
            assert_eq!(looped.id, 0);

            // RepeatMode::Single の動作（次曲でも同じ曲）
            pm.set_repeat_mode(RepeatMode::Single);
            let track1_path = pm.all_tracks()[1].path.clone();
            pm.select_and_play_path(&track1_path);
            let same = pm.next_track().unwrap();
            assert_eq!(same.id, 1);

            // シャッフル動作
            pm.set_repeat_mode(RepeatMode::Off);
            pm.set_shuffle(true);
            assert!(pm.is_shuffle());

            let mut visited = Vec::new();
            let first = pm.select_and_play_path(&pm.all_tracks()[0].path.clone()).unwrap();
            visited.push(first.id);
            while let Some(t) = pm.next_track() {
                visited.push(t.id);
            }
            assert_eq!(visited.len(), count);
            // 重複がないことを検証
            let mut dedupped = visited.clone();
            dedupped.sort();
            dedupped.dedup();
            assert_eq!(dedupped.len(), count);
        }
    }

    #[test]
    fn test_playlist_hierarchical_navigation() {
        let mut pm = PlaylistManager::new();
        let count = pm.load_path("sample");
        assert!(count > 0);

        // 親ディレクトリで1曲目を再生
        let playing_track = pm.select_and_play(0).unwrap();
        let playing_path = playing_track.path.clone();
        assert_eq!(pm.current_track_path(), Some(&playing_path));

        // sample 直下に sample-child サブディレクトリがあるはず
        let sub_dir_entry = pm.entries().iter().find(|e| matches!(e, PlaylistEntry::Directory { name, .. } if name.contains("sample-child")));
        if let Some(PlaylistEntry::Directory { path, .. }) = sub_dir_entry {
            let child_path = path.clone();
            // サブフォルダへ進入
            assert!(pm.enter_directory(&child_path));
            assert!(pm.breadcrumb().contains("sample-child"));

            // サブフォルダに入っても、親で再生中の current_track_path は不変！
            assert_eq!(pm.current_track_path(), Some(&playing_path));

            // サブフォルダ内には .. [PARENT DIR] が存在するはず
            assert_eq!(pm.entries().first(), Some(&PlaylistEntry::ParentDir));

            // 親フォルダへ戻る
            assert!(pm.go_to_parent());
            assert!(!pm.breadcrumb().contains("sample-child"));
            assert_eq!(pm.current_track_path(), Some(&playing_path));

            // ルートからの脱出防止（ルートで go_to_parent しても false）
            assert!(!pm.go_to_parent());
        }
    }
}
