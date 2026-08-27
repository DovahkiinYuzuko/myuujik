use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "m4a", "aac", "flac", "alac", "ogg", "opus",
];

pub struct AudioScanner;

impl AudioScanner {
    pub fn is_supported_extension<P: AsRef<Path>>(path: P) -> bool {
        path.as_ref()
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext_str| {
                let lower = ext_str.to_lowercase();
                SUPPORTED_EXTENSIONS.contains(&lower.as_str())
            })
            .unwrap_or(false)
    }

    pub fn scan_path<P: AsRef<Path>>(target: P) -> Vec<PathBuf> {
        let path = target.as_ref();
        if !path.exists() {
            return Vec::new();
        }

        if path.is_file() {
            if Self::is_supported_extension(path) {
                return vec![path.to_path_buf()];
            } else {
                return Vec::new();
            }
        }

        let mut tracks = Vec::new();
        for entry in WalkDir::new(path).follow_links(true).into_iter().filter_map(|e| e.ok()) {
            let entry_path = entry.path();
            if entry_path.is_file() && Self::is_supported_extension(entry_path) {
                tracks.push(entry_path.to_path_buf());
            }
        }

        tracks.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
        tracks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_filter() {
        assert!(AudioScanner::is_supported_extension(Path::new("song.flac")));
        assert!(AudioScanner::is_supported_extension(Path::new("song.WAV")));
        assert!(AudioScanner::is_supported_extension(Path::new("song.mp3")));
        assert!(AudioScanner::is_supported_extension(Path::new("song.m4a")));
        assert!(!AudioScanner::is_supported_extension(Path::new("doc.pdf")));
        assert!(!AudioScanner::is_supported_extension(Path::new("notes.txt")));
    }

    #[test]
    fn test_scan_sample_directory() {
        let sample_dir = Path::new("sample");
        if sample_dir.exists() {
            let tracks = AudioScanner::scan_path(sample_dir);
            assert!(tracks.len() >= 3, "Expected at least 3 sample tracks, found {}", tracks.len());
            for t in &tracks {
                assert!(AudioScanner::is_supported_extension(t));
            }
        }
    }
}
