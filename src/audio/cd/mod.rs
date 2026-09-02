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
        if let Some(s) = p.to_str() {
            let trimmed = s.trim();
            // 例: "D:", "D:\", "\\.\D:"
            if trimmed.len() >= 2 && trimmed.chars().nth(1) == Some(':') {
                let drive_char = trimmed.chars().next().unwrap().to_ascii_uppercase();
                if ('A'..='Z').contains(&drive_char) {
                    if trimmed.len() <= 3 {
                        return windows::is_cdrom_drive(drive_char);
                    }
                }
            }
        }
    }

    false
}
