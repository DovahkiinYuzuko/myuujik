use std::path::{Path, PathBuf};

/// Pure Rust による超軽量 SHA-1 実装（依存クレートゼロ）
pub struct SimpleSha1 {
    h: [u32; 5],
    len: u64,
    buffer: [u8; 64],
    buf_len: usize,
}

impl SimpleSha1 {
    pub fn new() -> Self {
        Self {
            h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            len: 0,
            buffer: [0u8; 64],
            buf_len: 0,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.len += (data.len() as u64) * 8;
        let mut offset = 0;
        while offset < data.len() {
            let space = 64 - self.buf_len;
            let take = space.min(data.len() - offset);
            self.buffer[self.buf_len..self.buf_len + take].copy_from_slice(&data[offset..offset + take]);
            self.buf_len += take;
            offset += take;

            if self.buf_len == 64 {
                self.process_block();
                self.buf_len = 0;
            }
        }
    }

    pub fn finalize(mut self) -> [u8; 20] {
        self.buffer[self.buf_len] = 0x80;
        self.buf_len += 1;

        if self.buf_len > 56 {
            for b in &mut self.buffer[self.buf_len..64] {
                *b = 0;
            }
            self.process_block();
            self.buf_len = 0;
        }

        for b in &mut self.buffer[self.buf_len..56] {
            *b = 0;
        }
        self.buffer[56..64].copy_from_slice(&self.len.to_be_bytes());
        self.process_block();

        let mut digest = [0u8; 20];
        for (i, &val) in self.h.iter().enumerate() {
            digest[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
        }
        digest
    }

    fn process_block(&mut self) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                self.buffer[i * 4],
                self.buffer[i * 4 + 1],
                self.buffer[i * 4 + 2],
                self.buffer[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let mut a = self.h[0];
        let mut b = self.h[1];
        let mut c = self.h[2];
        let mut d = self.h[3];
        let mut e = self.h[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }
}

/// MusicBrainz 形式の URL セーフ Base64 エンコード
fn base64_url_safe(bytes: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(CHARSET[idx0] as char);
        out.push(CHARSET[idx1] as char);
        if i + 1 < bytes.len() {
            out.push(CHARSET[idx2] as char);
        } else {
            out.push('-');
        }
        if i + 2 < bytes.len() {
            out.push(CHARSET[idx3] as char);
        } else {
            out.push('-');
        }
        i += 3;
    }
    out
}

/// TOC から MusicBrainz DiscID を算出する（MusicBrainz 公式規格: 804文字 ASCII SHA-1）
pub fn calculate_musicbrainz_disc_id(
    first_track: u8,
    last_track: u8,
    leadout_lba: i32,
    track_lbas: &[i32],
) -> String {
    // 1. FirstTrack (2文字) + LastTrack (2文字) + Leadout (8文字)
    let mut s = format!("{:02X}{:02X}{:08X}", first_track, last_track, (leadout_lba + 150) as u32);
    // 2. 99個のトラックオフセット（各8文字）。スロットは合計で 1(Leadout) + 99 = 100スロット。
    for i in 0..99 {
        if i < track_lbas.len() {
            s.push_str(&format!("{:08X}", (track_lbas[i] + 150) as u32));
        } else {
            s.push_str("00000000");
        }
    }

    let mut hasher = SimpleSha1::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    let disc_id = base64_url_safe(&digest);
    crate::logger::info(
        "CdMetadata",
        &format!(
            "Calculated MusicBrainz DiscID: {} (first={}, last={}, leadout_lba={}, tracks_len={})",
            disc_id, first_track, last_track, leadout_lba, track_lbas.len()
        ),
    );
    disc_id
}

/// MusicBrainz クエリ用の TOC 文字列を生成する（例: "1 6 95462 150 15363 32314 46592 63414 80489"）
pub fn create_musicbrainz_toc_string(
    first_track: u8,
    last_track: u8,
    leadout_lba: i32,
    track_lbas: &[i32],
) -> String {
    let mut toc = format!("{} {} {}", first_track, last_track, leadout_lba + 150);
    for &lba in track_lbas {
        toc.push_str(&format!(" {}", lba + 150));
    }
    toc
}

/// myuujik の CD カバーアートキャッシュディレクトリを取得（存在しない場合は作成）
pub fn get_cd_cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let mut p = PathBuf::from(local_app_data);
            p.push("myuujik");
            p.push("cache");
            p.push("albumart");
            let _ = std::fs::create_dir_all(&p);
            return Some(p);
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let mut p = PathBuf::from(home);
            p.push(".cache");
            p.push("myuujik");
            p.push("albumart");
            let _ = std::fs::create_dir_all(&p);
            return Some(p);
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CdTrackMetadata {
    pub track_number: u8,
    pub title: String,
    pub artist: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CdAlbumMetadata {
    pub album_title: String,
    pub artist: String,
    pub tracks: Vec<CdTrackMetadata>,
}

/// DiscID からキャッシュ画像ファイルパスを取得する
pub fn get_cached_cover_art_path(disc_id: &str) -> Option<PathBuf> {
    let sanitized_id: String = disc_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let mut path = get_cd_cache_dir()?;
    path.push(format!("{}.jpg", sanitized_id));
    Some(path)
}

/// DiscID からキャッシュメタデータ JSON ファイルパスを取得する
pub fn get_cached_metadata_path(disc_id: &str) -> Option<PathBuf> {
    let sanitized_id: String = disc_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let mut path = get_cd_cache_dir()?;
    path.push(format!("{}.json", sanitized_id));
    Some(path)
}

/// キャッシュ済みの CD アルバムメタデータを読み込む
pub fn load_cached_cd_metadata(disc_id: &str) -> Option<CdAlbumMetadata> {
    let path = get_cached_metadata_path(disc_id)?;
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(meta) = serde_json::from_str::<CdAlbumMetadata>(&content) {
                return Some(meta);
            }
        }
    }
    None
}

/// MusicBrainz API および Cover Art Archive を経由してジャケット写真およびメタデータを同期取得・キャッシュ保存する
pub fn fetch_cover_art_from_musicbrainz(disc_id: &str, toc_string: Option<&str>) -> Option<PathBuf> {
    let cache_path = get_cached_cover_art_path(disc_id)?;
    let metadata_path = get_cached_metadata_path(disc_id);

    let has_image = cache_path.exists() && std::fs::metadata(&cache_path).map(|m| m.len() > 0).unwrap_or(false);
    let has_metadata = metadata_path.as_ref().map(|p| p.exists()).unwrap_or(false);

    if has_image && has_metadata {
        crate::logger::info("CdMetadata", &format!("Using cached album art and metadata for DiscID: {} ({:?})", disc_id, cache_path));
        return Some(cache_path);
    }

    crate::logger::info("CdMetadata", &format!("Starting cover art & metadata resolution for DiscID: {} (toc: {:?})", disc_id, toc_string));

    // 1. MusicBrainz API に問い合わせて Release MBID およびメタデータを取得
    let mb_url = match toc_string {
        Some(toc) => format!("https://musicbrainz.org/ws/2/discid/{}?toc={}&inc=recordings+artist-credits&fmt=json", disc_id, toc.replace(' ', "+")),
        None => format!("https://musicbrainz.org/ws/2/discid/{}?inc=recordings+artist-credits&fmt=json", disc_id),
    };
    let user_agent = "myuujik/0.1.0 ( rikuichi0212@gmail.com )";

    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };

    crate::logger::info("CdMetadata", &format!("Requesting MusicBrainz: url={}", mb_url));

    let mb_output = match std::process::Command::new(curl_bin)
        .args([
            "-s",
            "-A",
            user_agent,
            "--max-time",
            "5",
            &mb_url,
        ])
        .output() {
            Ok(out) => out,
            Err(e) => {
                crate::logger::error("CdMetadata", &format!("Failed to execute curl for MusicBrainz: {}", e));
                return None;
            }
        };

    let status_code = mb_output.status.code().unwrap_or(-1);
    let json_text = String::from_utf8_lossy(&mb_output.stdout);
    crate::logger::info("CdMetadata", &format!("MusicBrainz response: exit_code={}, stdout_len={}", status_code, json_text.len()));

    if !mb_output.status.success() || json_text.trim().is_empty() {
        crate::logger::warn("CdMetadata", &format!("MusicBrainz query unsuccessful: exit_code={}, response={}", status_code, json_text.trim()));
        return None;
    }

    let parsed: serde_json::Value = match serde_json::from_str(&json_text) {
        Ok(v) => v,
        Err(e) => {
            crate::logger::warn("CdMetadata", &format!("Failed to parse MusicBrainz JSON: {} (response={})", e, json_text.trim()));
            return None;
        }
    };

    if let Some(err_msg) = parsed.get("error").and_then(|e| e.as_str()) {
        crate::logger::warn("CdMetadata", &format!("MusicBrainz API returned error: {}", err_msg));
        return None;
    }

    let releases = match parsed.get("releases").and_then(|r| r.as_array()) {
        Some(r) if !r.is_empty() => r,
        _ => {
            crate::logger::warn("CdMetadata", &format!("No matching releases found on MusicBrainz for DiscID: {}", disc_id));
            return None;
        }
    };

    let release = &releases[0];
    let mbid = match release.get("id").and_then(|id| id.as_str()) {
        Some(id) => id,
        None => {
            crate::logger::warn("CdMetadata", "Release object missing 'id' field");
            return None;
        }
    };
    let release_title = release.get("title").and_then(|t| t.as_str()).unwrap_or("Unknown Album").to_string();
    let release_artist = release.get("artist-credit")
        .and_then(|ac| ac.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Audio CD")
        .to_string();

    crate::logger::info("CdMetadata", &format!("Matched MusicBrainz Release: title='{}', artist='{}', mbid={}", release_title, release_artist, mbid));

    // 各トラック情報の抽出
    let mut tracks_meta = Vec::new();
    if let Some(media) = release.get("media").and_then(|m| m.as_array()) {
        if let Some(first_media) = media.first() {
            if let Some(track_list) = first_media.get("tracks").and_then(|t| t.as_array()) {
                for (idx, t) in track_list.iter().enumerate() {
                    let track_num = t.get("number")
                        .and_then(|n| n.as_str())
                        .and_then(|s| s.parse::<u8>().ok())
                        .unwrap_or((idx + 1) as u8);
                    let title = t.get("title")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let artist = t.get("artist-credit")
                        .and_then(|ac| ac.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|item| item.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());

                    if !title.is_empty() {
                        tracks_meta.push(CdTrackMetadata {
                            track_number: track_num,
                            title,
                            artist,
                        });
                    }
                }
            }
        }
    }

    let album_meta = CdAlbumMetadata {
        album_title: release_title,
        artist: release_artist,
        tracks: tracks_meta,
    };

    if let Some(json_path) = metadata_path {
        if let Ok(json_str) = serde_json::to_string_pretty(&album_meta) {
            let _ = std::fs::write(&json_path, json_str);
            crate::logger::info("CdMetadata", &format!("Saved CD album metadata to: {:?}", json_path));
        }
    }

    // 既に画像がある場合はダウンロードをスキップ
    if has_image {
        return Some(cache_path);
    }

    // 2. Cover Art Archive からフロントカバー画像を取得
    let caa_url = format!("https://coverartarchive.org/release/{}/front-250", mbid);
    let temp_path = cache_path.with_extension("tmp");

    crate::logger::info("CdMetadata", &format!("Requesting Cover Art Archive: url={}", caa_url));

    let caa_output = match std::process::Command::new(curl_bin)
        .args([
            "-s",
            "-L", // リダイレクト追従
            "-A",
            user_agent,
            "--max-time",
            "10",
            "-o",
            temp_path.to_str()?,
            &caa_url,
        ])
        .output() {
            Ok(out) => out,
            Err(e) => {
                crate::logger::error("CdMetadata", &format!("Failed to execute curl for Cover Art Archive: {}", e));
                let _ = std::fs::remove_file(&temp_path);
                return None;
            }
        };

    let caa_status = caa_output.status.code().unwrap_or(-1);
    if !caa_output.status.success() || !temp_path.exists() {
        crate::logger::warn("CdMetadata", &format!("Cover Art Archive query failed: exit_code={}", caa_status));
        let _ = std::fs::remove_file(&temp_path);
        return None;
    }

    if let Ok(meta) = std::fs::metadata(&temp_path) {
        if meta.len() > 0 {
            if std::fs::rename(&temp_path, &cache_path).is_ok() {
                crate::logger::info("CdMetadata", &format!("Successfully downloaded and cached cover art: size={} bytes, path={:?}", meta.len(), cache_path));
                return Some(cache_path);
            }
        }
    }

    crate::logger::warn("CdMetadata", "Downloaded cover art file was empty or failed to rename");
    let _ = std::fs::remove_file(&temp_path);
    None
}

/// バックグラウンドスレッドで MusicBrainz からのジャケット写真取得を開始する
pub fn trigger_cd_cover_art_fetch(disc_id: &str, toc_string: Option<&str>) {
    if let Some(path) = get_cached_cover_art_path(disc_id) {
        if path.exists() {
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > 0 {
                    return; // 既にキャッシュ済み
                }
            }
        }
    }

    let disc_id_owned = disc_id.to_string();
    let toc_owned = toc_string.map(|s| s.to_string());
    std::thread::spawn(move || {
        crate::logger::info("CdMetadata", &format!("Background cover art fetch started for DiscID: {}", disc_id_owned));
        let _ = fetch_cover_art_from_musicbrainz(&disc_id_owned, toc_owned.as_deref());
    });
}

/// CD ドライブ内または周辺キャッシュ、オンラインからのアルバムアート画像探索
pub fn find_cd_album_art(drive_letter: char, disc_id: Option<&str>, toc_string: Option<&str>) -> Option<(String, Vec<u8>)> {
    // 0. ローカルキャッシュに存在するか確認
    if let Some(did) = disc_id {
        if let Some(cache_path) = get_cached_cover_art_path(did) {
            if cache_path.exists() {
                if let Ok(data) = std::fs::read(&cache_path) {
                    if !data.is_empty() {
                        crate::logger::info("CdMetadata", &format!("Loaded cover art from local cache for DiscID: {}", did));
                        return Some(("image/jpeg".to_string(), data));
                    }
                }
            }
        }
        // キャッシュに無ければバックグラウンドでオンライン取得を発火
        trigger_cd_cover_art_fetch(did, toc_string);
    }

    let drive_root = format!("{}:\\", drive_letter);
    let root_path = Path::new(&drive_root);

    // 1. ドライブ直下の画像（隠し属性含む）
    let candidate_names = [
        "Folder.jpg", "folder.jpg", "cover.jpg", "Cover.jpg",
        "AlbumArtSmall.jpg", "AlbumArt_{*.jpg", "front.jpg", "front.png",
    ];

    if let Ok(entries) = std::fs::read_dir(root_path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(file_name) = p.file_name().and_then(|f| f.to_str()) {
                for &cand in &candidate_names {
                    if file_name.eq_ignore_ascii_case(cand) {
                        if let Ok(data) = std::fs::read(&p) {
                            let mime = if file_name.to_lowercase().ends_with(".png") {
                                "image/png".to_string()
                            } else {
                                "image/jpeg".to_string()
                            };
                            return Some((mime, data));
                        }
                    }
                }
            }
        }
    }

    // 2. Windows Media Player のローカルキャッシュフォルダ探索
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let mut cache_dir = PathBuf::from(local_app_data);
            cache_dir.push("Microsoft");
            cache_dir.push("Media Player");
            cache_dir.push("アートワークのキャッシュ");
            if cache_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&cache_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                            if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
                                if let Ok(data) = std::fs::read(&p) {
                                    return Some(("image/jpeg".to_string(), data));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_sha1_known_vector() {
        let mut hasher = SimpleSha1::new();
        hasher.update(b"The quick brown fox jumps over the lazy dog");
        let digest = hasher.finalize();
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(hex, "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12");
    }

    #[test]
    fn test_musicbrainz_disc_id_format() {
        let disc_id = calculate_musicbrainz_disc_id(1, 10, 150000, &[0, 15000, 30000, 45000, 60000, 75000, 90000, 105000, 120000, 135000]);
        assert_eq!(disc_id.len(), 28);
        assert!(!disc_id.contains('/'));
        assert!(!disc_id.contains('+'));
    }

    #[test]
    fn test_musicbrainz_disc_id_official_vector() {
        // MusicBrainz 公式ドキュメント記載のテストベクトル:
        // https://musicbrainz.org/doc/DiscIDCalculation
        // FirstTrack: 1, LastTrack: 6, Leadout: 95312 (LBA) -> offset = 95462
        // Track 1..6 LBAs: [0, 15213, 32164, 46442, 63264, 80339] -> offsets = [150, 15363, 32314, 46592, 63414, 80489]
        // 期待される DiscID: "49HHV7Eb8UKF3aQiNmu1GR8vKTY-"
        let disc_id = calculate_musicbrainz_disc_id(
            1,
            6,
            95312,
            &[0, 15213, 32164, 46442, 63264, 80339],
        );
        assert_eq!(disc_id, "49HHV7Eb8UKF3aQiNmu1GR8vKTY-");

        let toc = create_musicbrainz_toc_string(
            1,
            6,
            95312,
            &[0, 15213, 32164, 46442, 63264, 80339],
        );
        assert_eq!(toc, "1 6 95462 150 15363 32314 46592 63414 80489");
    }

    #[test]
    fn test_get_cached_cover_art_path() {
        let path = get_cached_cover_art_path("test-disc_id.123");
        assert!(path.is_some());
        let p = path.unwrap();
        assert!(p.to_string_lossy().ends_with("test-disc_id.123.jpg"));
    }

    #[test]
    fn test_cd_album_metadata_serialization() {
        let meta = CdAlbumMetadata {
            album_title: "PARADE".to_string(),
            artist: "Test Artist".to_string(),
            tracks: vec![
                CdTrackMetadata {
                    track_number: 1,
                    title: "Opening".to_string(),
                    artist: None,
                },
                CdTrackMetadata {
                    track_number: 2,
                    title: "Main Theme".to_string(),
                    artist: Some("Guest Artist".to_string()),
                },
            ],
        };

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: CdAlbumMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, deserialized);
    }
}
