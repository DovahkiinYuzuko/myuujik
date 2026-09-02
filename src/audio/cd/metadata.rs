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

/// TOC から MusicBrainz DiscID を算出する
pub fn calculate_musicbrainz_disc_id(
    first_track: u8,
    last_track: u8,
    leadout_lba: i32,
    track_lbas: &[i32],
) -> String {
    let mut s = format!("{:02X}{:02X}{:08X}", first_track, last_track, (leadout_lba + 150) as u32);
    for i in 0..100 {
        if i < track_lbas.len() {
            s.push_str(&format!("{:08X}", (track_lbas[i] + 150) as u32));
        } else {
            s.push_str("00000000");
        }
    }

    let mut hasher = SimpleSha1::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    base64_url_safe(&digest)
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

/// MusicBrainz API および Cover Art Archive を経由してジャケット写真を同期取得・キャッシュ保存する
pub fn fetch_cover_art_from_musicbrainz(disc_id: &str) -> Option<PathBuf> {
    let cache_path = get_cached_cover_art_path(disc_id)?;
    if cache_path.exists() {
        if let Ok(meta) = std::fs::metadata(&cache_path) {
            if meta.len() > 0 {
                crate::logger::info("CdMetadata", &format!("Using cached album art for DiscID: {}", disc_id));
                return Some(cache_path);
            }
        }
    }

    crate::logger::info("CdMetadata", &format!("Fetching cover art from MusicBrainz for DiscID: {}", disc_id));

    // 1. MusicBrainz API に問い合わせて Release MBID を取得
    let mb_url = format!("https://musicbrainz.org/ws/2/discid/{}?fmt=json", disc_id);
    let user_agent = "myuujik/0.1.0 ( rikuichi0212@gmail.com )";

    let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };

    let mb_output = std::process::Command::new(curl_bin)
        .args([
            "-s",
            "-A",
            user_agent,
            "--max-time",
            "5",
            &mb_url,
        ])
        .output()
        .ok()?;

    if !mb_output.status.success() {
        crate::logger::warn("CdMetadata", &format!("MusicBrainz query failed for DiscID: {}", disc_id));
        return None;
    }

    let json_text = String::from_utf8_lossy(&mb_output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&json_text).ok()?;
    let mbid = parsed.get("releases")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()?;

    crate::logger::info("CdMetadata", &format!("Found MusicBrainz Release MBID: {}", mbid));

    // 2. Cover Art Archive からジャケット画像を取得
    let caa_url = format!("https://coverartarchive.org/release/{}/front-250", mbid);
    let temp_path = cache_path.with_extension("tmp");

    let caa_output = std::process::Command::new(curl_bin)
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
        .output()
        .ok()?;

    if !caa_output.status.success() || !temp_path.exists() {
        crate::logger::warn("CdMetadata", &format!("Cover Art Archive query failed for MBID: {}", mbid));
        let _ = std::fs::remove_file(&temp_path);
        return None;
    }

    if let Ok(meta) = std::fs::metadata(&temp_path) {
        if meta.len() > 0 {
            if std::fs::rename(&temp_path, &cache_path).is_ok() {
                crate::logger::info("CdMetadata", &format!("Successfully saved cover art to: {:?}", cache_path));
                return Some(cache_path);
            }
        }
    }

    let _ = std::fs::remove_file(&temp_path);
    None
}

/// バックグラウンドスレッドで MusicBrainz からのジャケット写真取得を開始する
pub fn trigger_cd_cover_art_fetch(disc_id: &str) {
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
    std::thread::spawn(move || {
        crate::logger::info("CdMetadata", &format!("Background cover art fetch started for DiscID: {}", disc_id_owned));
        let _ = fetch_cover_art_from_musicbrainz(&disc_id_owned);
    });
}

/// CD ドライブ内または周辺キャッシュ、オンラインからのアルバムアート画像探索
pub fn find_cd_album_art(drive_letter: char, disc_id: Option<&str>) -> Option<(String, Vec<u8>)> {
    // 0. ローカルキャッシュに存在するか確認
    if let Some(did) = disc_id {
        if let Some(cache_path) = get_cached_cover_art_path(did) {
            if cache_path.exists() {
                if let Ok(data) = std::fs::read(&cache_path) {
                    if !data.is_empty() {
                        return Some(("image/jpeg".to_string(), data));
                    }
                }
            }
        }
        // キャッシュに無ければバックグラウンドでオンライン取得を発火
        trigger_cd_cover_art_fetch(did);
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
    fn test_get_cached_cover_art_path() {
        let path = get_cached_cover_art_path("test-disc_id.123");
        assert!(path.is_some());
        let p = path.unwrap();
        assert!(p.to_string_lossy().ends_with("test-disc_id.123.jpg"));
    }
}
