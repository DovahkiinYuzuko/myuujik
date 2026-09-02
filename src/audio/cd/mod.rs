pub mod metadata;

#[cfg(windows)]
pub mod windows;

#[cfg(not(windows))]
pub mod unix;

use std::error::Error;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct CdTrackInfo {
    pub track_number: u8,
    pub start_lba: i32,
    pub sector_count: u32,
    pub duration_secs: f64,
    pub is_audio: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CdDiscInfo {
    pub drive_letter: char,
    pub first_track: u8,
    pub last_track: u8,
    pub leadout_lba: i32,
    pub disc_id: String,
    pub toc_string: String,
    pub tracks: Vec<CdTrackInfo>,
}

pub trait CdReader: Send + Sync {
    /// ディスク全体のTOCおよびトラック情報を取得する
    fn read_disc_info(&mut self) -> Result<CdDiscInfo, Box<dyn Error + Send + Sync>>;

    /// 再生対象のトラック（1-indexed）を選択し、再生ヘッドをそのトラックの先頭LBAに移動する
    fn set_track(&mut self, track_number: u8) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// 現在のトラック内の指定秒数位置へシークする
    fn seek(&mut self, target_secs: f64) -> Result<f64, Box<dyn Error + Send + Sync>>;

    /// 次のPCMオーディオパケット（f32インターリーブサンプル、ステレオ44.1kHz）を読み出す
    fn read_next_packet(&mut self) -> Result<Option<Vec<f32>>, Box<dyn Error + Send + Sync>>;
}

/// パスからドライブ文字（'A'..='Z'）を安全に抽出する（\\?\D:\ や \\.\D: 等のプレフィックスに対応）
pub fn extract_drive_letter<P: AsRef<Path>>(path: P) -> Option<char> {
    use std::path::{Component, Prefix};

    for comp in path.as_ref().components() {
        if let Component::Prefix(prefix) = comp {
            match prefix.kind() {
                Prefix::Disk(c) | Prefix::VerbatimDisk(c) => {
                    return Some((c as char).to_ascii_uppercase());
                }
                _ => {}
            }
        }
    }

    // フォールバック（文字列スキャン: 例 "D:" や "\\.\D:"）
    let s = path.as_ref().to_string_lossy();
    for (idx, ch) in s.char_indices() {
        if ch == ':' && idx > 0 {
            if let Some(prev) = s[..idx].chars().last() {
                if prev.is_ascii_alphabetic() {
                    return Some(prev.to_ascii_uppercase());
                }
            }
        }
    }

    None
}

/// 指定されたパスがCDドライブまたはCDAトラックであるかを判定する
pub fn is_cd_path<P: AsRef<Path>>(path: P) -> bool {
    let p = path.as_ref();
    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
        if ext.eq_ignore_ascii_case("cda") {
            return true;
        }
    }

    #[cfg(windows)]
    {
        if let Some(drive_char) = extract_drive_letter(p) {
            let s = p.to_string_lossy();
            let stripped = s.trim_start_matches(r"\\?\").trim();
            if stripped.len() <= 3 {
                return windows::is_cdrom_drive(drive_char);
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_drive_letter() {
        assert_eq!(extract_drive_letter(r"D:\"), Some('D'));
        assert_eq!(extract_drive_letter(r"\\?\D:\Track01.cda"), Some('D'));
        assert_eq!(extract_drive_letter(r"\\.\E:"), Some('E'));
        assert_eq!(extract_drive_letter("f:/songs/track.cda"), Some('F'));
        assert_eq!(extract_drive_letter("relative/path/song.mp3"), None);
    }
}
