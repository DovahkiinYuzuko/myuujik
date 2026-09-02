use std::error::Error;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;

use super::metadata::calculate_musicbrainz_disc_id;
use super::{CdDiscInfo, CdReader, CdTrackInfo};

const IOCTL_CDROM_READ_TOC: u32 = 0x00024000;
const IOCTL_CDROM_RAW_READ: u32 = 0x0002403E;
const CD_RAW_SECTOR_SIZE: usize = 2352;
const SECTORS_PER_READ: u32 = 8; // 1回のIOCTLで8セクタ(約106ms分)をまとめてストリーミング読み出し
const TRACK_MODE_CDDA: u32 = 2; // TRACK_MODE_TYPE::CDDA = 2 (0=YellowMode2, 1=XAForm2)

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct TrackData {
    reserved: u8,
    control_and_adr: u8,
    track_number: u8,
    reserved1: u8,
    address: [u8; 4], // [0] = 0, [1] = M, [2] = S, [3] = F
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CdromToc {
    length: [u8; 2],
    first_track: u8,
    last_track: u8,
    track_data: [TrackData; 100],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct RawReadInfo {
    disk_offset: i64,
    sector_count: u32,
    track_mode: u32, // 0 = CDDA
}

pub struct WindowsCdReader {
    drive_letter: char,
    handle: HANDLE,
    disc_info: Option<CdDiscInfo>,
    current_track: u8,
    current_lba: i32,
    track_start_lba: i32,
    track_end_lba: i32,
}

unsafe impl Send for WindowsCdReader {}
unsafe impl Sync for WindowsCdReader {}

impl WindowsCdReader {
    /// 指定されたパス（例: "D:", "D:\\", "D:\\Track01.cda"）から CD ドライブリーダーを開く
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let drive_char = super::extract_drive_letter(path.as_ref())
            .ok_or("Invalid path: could not determine drive letter for CD drive")?;

        if !is_cdrom_drive(drive_char) {
            return Err(format!("Drive {}: is not a CD-ROM / DVD optical drive", drive_char).into());
        }

        let device_path = format!("\\\\.\\{}:", drive_char);
        let wide_path: Vec<u16> = std::ffi::OsStr::new(&device_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                FILE_GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                HANDLE::default(),
            )?
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(format!("Failed to open CD drive handle for {}:", drive_char).into());
        }

        let mut reader = Self {
            drive_letter: drive_char,
            handle,
            disc_info: None,
            current_track: 1,
            current_lba: 0,
            track_start_lba: 0,
            track_end_lba: 0,
        };

        // TOC の事前取得
        let disc_info = reader.read_disc_info()?;
        reader.disc_info = Some(disc_info);

        // 指定パスが特定のトラック（例: Track02.cda）を指している場合はそのトラックを選択
        let mut target_track = 1;
        if let Some(file_name) = path.as_ref().file_name().and_then(|f| f.to_str()) {
            let digits: String = file_name.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(num) = digits.parse::<u8>() {
                if num >= 1 {
                    target_track = num;
                }
            }
        }
        reader.set_track(target_track)?;

        Ok(reader)
    }

    pub fn disc_info(&self) -> Option<&CdDiscInfo> {
        self.disc_info.as_ref()
    }
}

impl Drop for WindowsCdReader {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

impl CdReader for WindowsCdReader {
    fn read_disc_info(&mut self) -> Result<CdDiscInfo, Box<dyn Error + Send + Sync>> {
        let mut toc = CdromToc {
            length: [0; 2],
            first_track: 0,
            last_track: 0,
            track_data: [TrackData {
                reserved: 0,
                control_and_adr: 0,
                track_number: 0,
                reserved1: 0,
                address: [0; 4],
            }; 100],
        };

        let mut bytes_returned = 0u32;
        let success = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_CDROM_READ_TOC,
                None,
                0,
                Some(&mut toc as *mut _ as *mut c_void),
                std::mem::size_of::<CdromToc>() as u32,
                Some(&mut bytes_returned),
                None,
            )
        };

        if success.is_err() {
            return Err("Failed to read CD Table of Contents (TOC). Is an Audio CD inserted?".into());
        }

        let first = toc.first_track;
        let last = toc.last_track;
        if first == 0 || last == 0 || first > last {
            return Err("Invalid CD TOC: No audio tracks found on disc".into());
        }

        let mut tracks = Vec::new();
        let mut track_lbas = Vec::new();

        let track_count = (last - first + 1) as usize;
        for i in 0..=track_count {
            let td = &toc.track_data[i];
            let m = td.address[1] as i32;
            let s = td.address[2] as i32;
            let f = td.address[3] as i32;
            let lba = (m * 60 + s) * 75 + f - 150;
            track_lbas.push(lba);
        }

        let leadout_lba = *track_lbas.last().unwrap_or(&0);

        for i in 0..track_count {
            let td = &toc.track_data[i];
            let start = track_lbas[i];
            let next = track_lbas[i + 1];
            let sectors = (next - start).max(0) as u32;
            let duration_secs = sectors as f64 / 75.0;
            let is_audio = (td.control_and_adr & 0x04) == 0;

            tracks.push(CdTrackInfo {
                track_number: td.track_number,
                start_lba: start,
                sector_count: sectors,
                duration_secs,
                is_audio,
            });
        }

        let disc_id = calculate_musicbrainz_disc_id(
            first,
            last,
            leadout_lba,
            &track_lbas[..track_count],
        );

        Ok(CdDiscInfo {
            drive_letter: self.drive_letter,
            first_track: first,
            last_track: last,
            leadout_lba,
            disc_id,
            tracks,
        })
    }

    fn set_track(&mut self, track_number: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
        let disc = self.disc_info.as_ref().ok_or("CD TOC not loaded")?;
        let t = disc
            .tracks
            .iter()
            .find(|t| t.track_number == track_number)
            .ok_or_else(|| format!("Track {} not found on CD", track_number))?;

        if !t.is_audio {
            return Err(format!("Track {} is a data track, not an audio track", track_number).into());
        }

        self.current_track = track_number;
        self.track_start_lba = t.start_lba;
        self.track_end_lba = t.start_lba + t.sector_count as i32;
        self.current_lba = t.start_lba;

        Ok(())
    }

    fn seek(&mut self, target_secs: f64) -> Result<f64, Box<dyn Error + Send + Sync>> {
        let target_lba_offset = (target_secs.max(0.0) * 75.0) as i32;
        let new_lba = (self.track_start_lba + target_lba_offset).min(self.track_end_lba);
        self.current_lba = new_lba;
        let actual_secs = (new_lba - self.track_start_lba) as f64 / 75.0;
        Ok(actual_secs)
    }

    fn read_next_packet(&mut self) -> Result<Option<Vec<f32>>, Box<dyn Error + Send + Sync>> {
        if self.current_lba >= self.track_end_lba {
            return Ok(None); // トラック終了
        }

        let remaining_sectors = (self.track_end_lba - self.current_lba) as u32;
        let sectors_to_read = remaining_sectors.min(SECTORS_PER_READ);

        let raw_info = RawReadInfo {
            disk_offset: (self.current_lba as i64) * 2048,
            sector_count: sectors_to_read,
            track_mode: TRACK_MODE_CDDA,
        };

        let buffer_size = (sectors_to_read as usize) * CD_RAW_SECTOR_SIZE;
        let mut raw_bytes = vec![0u8; buffer_size];
        let mut bytes_returned = 0u32;

        let success = unsafe {
            DeviceIoControl(
                self.handle,
                IOCTL_CDROM_RAW_READ,
                Some(&raw_info as *const _ as *const c_void),
                std::mem::size_of::<RawReadInfo>() as u32,
                Some(raw_bytes.as_mut_ptr() as *mut c_void),
                buffer_size as u32,
                Some(&mut bytes_returned),
                None,
            )
        };

        if let Err(e) = success {
            let win_err = windows::core::Error::from_win32();
            return Err(format!(
                "Failed to read raw CDDA sectors from optical drive at LBA {} (count={}): {} (Win32 code: {})",
                self.current_lba, sectors_to_read, e, win_err.code().0
            ).into());
        }

        let actual_bytes = bytes_returned as usize;
        if actual_bytes == 0 {
            return Ok(None);
        }

        let sectors_read = actual_bytes / CD_RAW_SECTOR_SIZE;
        self.current_lba += sectors_read as i32;

        // 16-bit Signed LE PCM -> f32 インターリーブサンプルへの変換
        let sample_count = actual_bytes / 2;
        let mut samples = Vec::with_capacity(sample_count);

        for chunk in raw_bytes[..actual_bytes].chunks_exact(2) {
            let s16 = i16::from_le_bytes([chunk[0], chunk[1]]);
            samples.push(s16 as f32 / 32768.0);
        }

        Ok(Some(samples))
    }
}

/// 指定したドライブ文字がCD-ROMドライブであるかを判定する
pub fn is_cdrom_drive(drive_letter: char) -> bool {
    let root = format!("{}:\\", drive_letter);
    let wide_root: Vec<u16> = std::ffi::OsStr::new(&root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide_root.as_ptr())) };
    drive_type == 5 // DRIVE_CDROM = 5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_read_info_layout() {
        assert_eq!(std::mem::size_of::<RawReadInfo>(), 16);
        assert_eq!(std::mem::align_of::<RawReadInfo>(), 8);
        assert_eq!(TRACK_MODE_CDDA, 2);
    }
}

