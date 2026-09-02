use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistItem {
    pub id: usize,
    pub path: PathBuf,
    pub display_name: String,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistEntry {
    ParentDir,
    Directory { name: String, path: PathBuf },
    AudioFile(PlaylistItem),
}

impl PlaylistEntry {
    pub fn display_name(&self) -> &str {
        match self {
            PlaylistEntry::ParentDir => ".. [PARENT DIR]",
            PlaylistEntry::Directory { name, .. } => name.as_str(),
            PlaylistEntry::AudioFile(item) => item.display_name.as_str(),
        }
    }

    pub fn is_audio_file(&self) -> bool {
        matches!(self, PlaylistEntry::AudioFile(_))
    }

    pub fn audio_item(&self) -> Option<&PlaylistItem> {
        match self {
            PlaylistEntry::AudioFile(item) => Some(item),
            _ => None,
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
