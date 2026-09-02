use crate::audio::decoder::CoverArt;
use std::path::Path;

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

/// 対象ファイルが動画形式であるかを判定する
pub fn is_video_file<P: AsRef<Path>>(path: P) -> bool {
    let p = path.as_ref();
    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp4" | "m4v" | "webm" | "mkv" | "mov" | "avi" | "flv" | "wmv"
        )
    } else {
        false
    }
}

/// プラットフォーム共通の動画サムネイル抽出インターフェース
pub fn extract_video_thumbnail<P: AsRef<Path>>(video_path: P) -> Option<CoverArt> {
    let p = video_path.as_ref();
    if !is_video_file(p) {
        return None;
    }

    #[cfg(windows)]
    {
        windows::extract_thumbnail(p)
    }

    #[cfg(target_os = "linux")]
    {
        linux::extract_thumbnail(p)
    }

    #[cfg(target_os = "macos")]
    {
        macos::extract_thumbnail(p)
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = p;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_video_file() {
        assert!(is_video_file("test.mp4"));
        assert!(is_video_file("test.MP4"));
        assert!(is_video_file("video.webm"));
        assert!(is_video_file("movie.mkv"));
        assert!(is_video_file("clip.mov"));
        assert!(!is_video_file("audio.mp3"));
        assert!(!is_video_file("track.flac"));
        assert!(!is_video_file("image.jpg"));
        assert!(!is_video_file("no_ext"));
    }
}
