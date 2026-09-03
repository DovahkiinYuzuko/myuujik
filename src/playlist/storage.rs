use std::fs;
use std::path::{Path, PathBuf};
use crate::playlist::m3u;
use crate::playlist::item::PlaylistItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomPlaylistInfo {
    pub name: String,
    pub path: PathBuf,
    pub track_count: usize,
}

#[derive(Debug, Clone)]
pub struct PlaylistStorage {
    base_dir: PathBuf,
}

impl PlaylistStorage {
    /// デフォルトの保存先（カレントディレクトリ配下の `playlists/`）で初期化
    pub fn new() -> Self {
        let base_dir = PathBuf::from("playlists");
        if !base_dir.exists() {
            let _ = fs::create_dir_all(&base_dir);
        }
        Self { base_dir }
    }

    /// 指定されたベースディレクトリで初期化（テスト・カスタム設定用）
    pub fn with_dir<P: AsRef<Path>>(dir: P) -> Self {
        let base_dir = dir.as_ref().to_path_buf();
        if !base_dir.exists() {
            let _ = fs::create_dir_all(&base_dir);
        }
        Self { base_dir }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// `playlists/` 配下の全 M3U / M3U8 ファイルを走査し、一覧を取得する
    pub fn list_playlists(&self) -> Vec<CustomPlaylistInfo> {
        let mut list = Vec::new();
        let read_dir = match fs::read_dir(&self.base_dir) {
            Ok(rd) => rd,
            Err(_) => return list,
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if ext == "m3u" || ext == "m3u8" {
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    let track_count = match fs::read_to_string(&path) {
                        Ok(content) => {
                            let parent = path.parent().unwrap_or(Path::new("."));
                            m3u::parse_m3u(&content, parent).len()
                        }
                        Err(_) => 0,
                    };
                    list.push(CustomPlaylistInfo {
                        name: stem,
                        path,
                        track_count,
                    });
                }
            }
        }

        list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        list
    }

    /// 指定された名前で現在のトラック一覧を EXTM3U8 形式で保存する
    pub fn save_playlist(&self, name: &str, tracks: &[PlaylistItem]) -> std::io::Result<PathBuf> {
        // ファイル名として安全な文字列にサニタイズ
        let safe_name: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
            .collect();
        let safe_name = if safe_name.trim().is_empty() {
            "Untitled".to_string()
        } else {
            safe_name.trim().to_string()
        };

        let file_name = format!("{}.m3u8", safe_name);
        let dest_path = self.base_dir.join(file_name);
        let m3u_str = m3u::export_m3u(tracks, Some(&self.base_dir));
        fs::write(&dest_path, m3u_str)?;
        Ok(dest_path)
    }

    /// 指定された名前のプレイリストファイルを削除する
    pub fn delete_playlist(&self, name: &str) -> std::io::Result<()> {
        let file_name = format!("{}.m3u8", name);
        let path = self.base_dir.join(&file_name);
        if path.exists() {
            fs::remove_file(path)?;
            return Ok(());
        }
        let file_name_m3u = format!("{}.m3u", name);
        let path_m3u = self.base_dir.join(file_name_m3u);
        if path_m3u.exists() {
            fs::remove_file(path_m3u)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_storage_lifecycle() {
        let temp_dir = std::env::temp_dir().join("myuujik_test_storage");
        let _ = fs::remove_dir_all(&temp_dir);
        let storage = PlaylistStorage::with_dir(&temp_dir);

        // 初期状態は空
        let list = storage.list_playlists();
        assert!(list.is_empty());

        // ダミートラック作成
        let t1 = PlaylistItem::from_path(0, "C:/Music/song1.mp3");
        let t2 = PlaylistItem::from_path(1, "C:/Music/song2.flac");

        // 保存
        let saved_path = storage.save_playlist("My Favorite J-Rock", &[t1, t2]).expect("Save should succeed");
        assert!(saved_path.exists());

        // 一覧取得
        let list = storage.list_playlists();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "My Favorite J-Rock");
        assert_eq!(list[0].track_count, 2);

        // 削除
        storage.delete_playlist("My Favorite J-Rock").expect("Delete should succeed");
        let list = storage.list_playlists();
        assert!(list.is_empty());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
