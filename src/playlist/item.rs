use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistItem {
    pub id: usize,
    pub path: PathBuf,
    pub display_name: String,
    pub duration_secs: Option<f64>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl PlaylistItem {
    pub fn from_path<P: AsRef<Path>>(id: usize, path: P) -> Self {
        let path_buf = path.as_ref().to_path_buf();
        let display_name = path_buf
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Track")
            .to_string();

        Self {
            id,
            path: path_buf,
            display_name,
            duration_secs: None,
            artist: None,
            album: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_item_from_path() {
        let item = PlaylistItem::from_path(1, Path::new("music/Cool Song.mp3"));
        assert_eq!(item.id, 1);
        assert_eq!(item.display_name, "Cool Song");
        assert_eq!(item.path, PathBuf::from("music/Cool Song.mp3"));
    }
}
