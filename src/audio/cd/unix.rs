use std::error::Error;
use super::{CdDiscInfo, CdReader};

pub struct UnixCdReader;

impl UnixCdReader {
    pub fn open(_device_path: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Err("CD-DA playback on Linux/macOS is currently a stub".into())
    }
}

impl CdReader for UnixCdReader {
    fn read_disc_info(&mut self) -> Result<CdDiscInfo, Box<dyn Error + Send + Sync>> {
        Err("CD-DA playback on Linux/macOS is not supported yet".into())
    }

    fn set_track(&mut self, _track_number: u8) -> Result<(), Box<dyn Error + Send + Sync>> {
        Err("CD-DA playback on Linux/macOS is not supported yet".into())
    }

    fn seek(&mut self, _target_secs: f64) -> Result<f64, Box<dyn Error + Send + Sync>> {
        Err("CD-DA playback on Linux/macOS is not supported yet".into())
    }

    fn read_next_packet(&mut self) -> Result<Option<Vec<f32>>, Box<dyn Error + Send + Sync>> {
        Err("CD-DA playback on Linux/macOS is not supported yet".into())
    }
}
