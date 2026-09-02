use crate::playlist::item::PlaylistItem;
use std::path::{Path, PathBuf};

/// M3U/M3U8 プレイリスト内の1曲を表す構造体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uEntry {
    pub path: PathBuf,
    pub title: Option<String>,
    pub duration_secs: Option<u32>,
}

/// M3U/M3U8 形式の文字列をパースし、楽曲エントリ一覧を取得する
pub fn parse_m3u(content: &str, base_dir: &Path) -> Vec<M3uEntry> {
    let mut entries = Vec::new();
    let mut pending_duration: Option<u32> = None;
    let mut pending_title: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("#EXTINF:") {
            let info_part = &line[8..];
            if let Some(comma_pos) = info_part.find(',') {
                let dur_str = info_part[..comma_pos].trim();
                let title_str = info_part[comma_pos + 1..].trim();

                if let Ok(dur) = dur_str.parse::<i64>() {
                    if dur > 0 {
                        pending_duration = Some(dur as u32);
                    }
                }
                if !title_str.is_empty() {
                    pending_title = Some(title_str.to_string());
                }
            }
            continue;
        }

        // その他のディレクティブ・コメント
        if line.starts_with('#') {
            continue;
        }

        // ファイルパス行
        let path_buf = PathBuf::from(line);
        let resolved_path = if path_buf.is_absolute() {
            path_buf
        } else {
            base_dir.join(path_buf)
        };

        let final_path = std::fs::canonicalize(&resolved_path).unwrap_or(resolved_path);

        entries.push(M3uEntry {
            path: final_path,
            title: pending_title.take(),
            duration_secs: pending_duration.take(),
        });
    }

    entries
}

/// 楽曲アイテム列から EXTM3U 形式のプレイリスト文字列を生成する
pub fn export_m3u(tracks: &[PlaylistItem], base_dir: Option<&Path>) -> String {
    let mut out = String::from("#EXTM3U\n");

    for track in tracks {
        out.push_str(&format!("#EXTINF:-1,{}\n", track.display_name));

        // 相対パス化が可能であれば相対パスで出力
        let path_str = if let Some(base) = base_dir {
            if let Ok(rel) = track.path.strip_prefix(base) {
                rel.to_string_lossy().to_string()
            } else {
                track.path.to_string_lossy().to_string()
            }
        } else {
            track.path.to_string_lossy().to_string()
        };

        out.push_str(&path_str);
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m3u_standard_and_extm3u() {
        let content = r#"#EXTM3U
#EXTINF:185,Artist A - Song One
song1.flac
# Comment line
#EXTINF:-1,Song Two No Artist
subfolder/song2.mp3
C:\Music\Absolute\song3.wav
"#;
        let base_dir = Path::new("C:/TestBase");
        let entries = parse_m3u(content, base_dir);

        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].title, Some("Artist A - Song One".to_string()));
        assert_eq!(entries[0].duration_secs, Some(185));

        assert_eq!(entries[1].title, Some("Song Two No Artist".to_string()));
        assert_eq!(entries[1].duration_secs, None);

        assert_eq!(entries[2].title, None);
        assert_eq!(entries[2].duration_secs, None);
        assert_eq!(entries[2].path, PathBuf::from(r"C:\Music\Absolute\song3.wav"));
    }

    #[test]
    fn test_export_m3u_roundtrip() {
        let items = vec![
            PlaylistItem {
                id: 0,
                path: PathBuf::from("track1.flac"),
                display_name: "Track One".to_string(),
            },
            PlaylistItem {
                id: 1,
                path: PathBuf::from("track2.mp3"),
                display_name: "Track Two".to_string(),
            },
        ];

        let m3u_str = export_m3u(&items, None);
        assert!(m3u_str.starts_with("#EXTM3U\n"));
        assert!(m3u_str.contains("#EXTINF:-1,Track One\ntrack1.flac\n"));
        assert!(m3u_str.contains("#EXTINF:-1,Track Two\ntrack2.mp3\n"));

        let base_dir = Path::new(".");
        let parsed = parse_m3u(&m3u_str, base_dir);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title.as_deref(), Some("Track One"));
        assert_eq!(parsed[1].title.as_deref(), Some("Track Two"));
    }
}
