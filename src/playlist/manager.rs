use crate::playlist::item::PlaylistItem;
use crate::playlist::scanner::AudioScanner;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    Single,
}

#[derive(Debug, Clone)]
pub struct PlaylistManager {
    items: Vec<PlaylistItem>,
    cursor: usize,
    current_playing_index: Option<usize>,
    repeat_mode: RepeatMode,
    shuffle_enabled: bool,
    shuffle_indices: Vec<usize>,
    shuffle_pos: usize,
}

impl PlaylistManager {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            cursor: 0,
            current_playing_index: None,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            shuffle_indices: Vec::new(),
            shuffle_pos: 0,
        }
    }

    pub fn load_path<P: AsRef<Path>>(&mut self, path: P) -> usize {
        let scanned_paths = AudioScanner::scan_path(path);
        self.items.clear();
        self.cursor = 0;
        self.current_playing_index = None;

        for (idx, p) in scanned_paths.into_iter().enumerate() {
            self.items.push(PlaylistItem::from_path(idx, p));
        }

        self.rebuild_shuffle_indices();
        self.items.len()
    }

    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn move_cursor_up(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        } else {
            self.cursor = self.items.len() - 1;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        if self.cursor + 1 < self.items.len() {
            self.cursor += 1;
        } else {
            self.cursor = 0;
        }
    }

    pub fn set_cursor(&mut self, index: usize) {
        if !self.items.is_empty() {
            self.cursor = index.min(self.items.len() - 1);
        }
    }

    pub fn selected_item(&self) -> Option<&PlaylistItem> {
        self.items.get(self.cursor)
    }

    pub fn current_track(&self) -> Option<&PlaylistItem> {
        self.current_playing_index.and_then(|idx| self.items.get(idx))
    }

    pub fn current_playing_index(&self) -> Option<usize> {
        self.current_playing_index
    }

    pub fn select_and_play(&mut self, index: usize) -> Option<&PlaylistItem> {
        if index < self.items.len() {
            self.current_playing_index = Some(index);
            self.cursor = index;

            // シャッフル順序の先頭を再生指定曲にして位置をリセット
            if self.shuffle_enabled {
                if let Some(pos) = self.shuffle_indices.iter().position(|&idx| idx == index) {
                    self.shuffle_indices.swap(0, pos);
                    self.shuffle_pos = 0;
                }
            }

            self.items.get(index)
        } else {
            None
        }
    }

    pub fn next_track(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }

        if self.repeat_mode == RepeatMode::Single {
            if let Some(idx) = self.current_playing_index {
                return self.items.get(idx);
            }
        }

        if self.shuffle_enabled && !self.shuffle_indices.is_empty() {
            if self.shuffle_pos + 1 < self.shuffle_indices.len() {
                self.shuffle_pos += 1;
                let next_idx = self.shuffle_indices[self.shuffle_pos];
                self.current_playing_index = Some(next_idx);
                self.cursor = next_idx;
                return self.items.get(next_idx);
            } else if self.repeat_mode == RepeatMode::All {
                self.reshuffle();
                let next_idx = self.shuffle_indices[0];
                self.current_playing_index = Some(next_idx);
                self.cursor = next_idx;
                return self.items.get(next_idx);
            } else {
                return None;
            }
        }

        // 通常順再生
        let next_idx = match self.current_playing_index {
            Some(curr) => {
                if curr + 1 < self.items.len() {
                    curr + 1
                } else if self.repeat_mode == RepeatMode::All {
                    0
                } else {
                    return None;
                }
            }
            None => 0,
        };

        self.current_playing_index = Some(next_idx);
        self.cursor = next_idx;
        self.items.get(next_idx)
    }

    pub fn prev_track(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }

        if self.shuffle_enabled && !self.shuffle_indices.is_empty() {
            if self.shuffle_pos > 0 {
                self.shuffle_pos -= 1;
                let prev_idx = self.shuffle_indices[self.shuffle_pos];
                self.current_playing_index = Some(prev_idx);
                self.cursor = prev_idx;
                return self.items.get(prev_idx);
            } else if self.repeat_mode == RepeatMode::All {
                self.shuffle_pos = self.shuffle_indices.len() - 1;
                let prev_idx = self.shuffle_indices[self.shuffle_pos];
                self.current_playing_index = Some(prev_idx);
                self.cursor = prev_idx;
                return self.items.get(prev_idx);
            } else {
                return self.current_track();
            }
        }

        // 通常順前曲
        let prev_idx = match self.current_playing_index {
            Some(curr) => {
                if curr > 0 {
                    curr - 1
                } else if self.repeat_mode == RepeatMode::All {
                    self.items.len() - 1
                } else {
                    0
                }
            }
            None => 0,
        };

        self.current_playing_index = Some(prev_idx);
        self.cursor = prev_idx;
        self.items.get(prev_idx)
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
        self.shuffle_indices = (0..self.items.len()).collect();
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
            assert_eq!(pm.len(), count);
            assert_eq!(pm.cursor(), 0);

            // カーソル移動巡回
            pm.move_cursor_down();
            assert_eq!(pm.cursor(), 1);
            pm.move_cursor_up();
            assert_eq!(pm.cursor(), 0);
            pm.move_cursor_up();
            assert_eq!(pm.cursor(), count - 1); // ループ

            // 再生開始
            let track0 = pm.select_and_play(0).unwrap();
            assert_eq!(track0.id, 0);

            // RepeatMode::Off の動作（終端でNone）
            pm.set_repeat_mode(RepeatMode::Off);
            pm.select_and_play(count - 1);
            assert!(pm.next_track().is_none());

            // RepeatMode::All の動作（終端で先頭へループ）
            pm.set_repeat_mode(RepeatMode::All);
            pm.select_and_play(count - 1);
            let looped = pm.next_track().unwrap();
            assert_eq!(looped.id, 0);

            // RepeatMode::Single の動作（次曲でも同じ曲）
            pm.set_repeat_mode(RepeatMode::Single);
            pm.select_and_play(1);
            let same = pm.next_track().unwrap();
            assert_eq!(same.id, 1);

            // シャッフル動作
            pm.set_repeat_mode(RepeatMode::Off);
            pm.set_shuffle(true);
            assert!(pm.is_shuffle());

            let mut visited = Vec::new();
            let first = pm.select_and_play(0).unwrap();
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
}
