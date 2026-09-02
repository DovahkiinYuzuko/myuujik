use crate::audio::decoder::CoverArt;
use std::path::{Path, PathBuf};

/// Linux Freedesktop Thumbnail Managing Standard に準拠したサムネイル抽出
pub fn extract_thumbnail<P: AsRef<Path>>(video_path: P) -> Option<CoverArt> {
    let p = video_path.as_ref();
    let abs_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let path_str = abs_path.to_str()?;

    // 1. URI の生成 (file:///path/to/video)
    let uri = format!("file://{}", path_str);

    // 2. URI 文字列の MD5 ハッシュを算出 (md5sum コマンドまたはフォールバック)
    let md5_hash = compute_uri_md5(&uri)?;

    // 3. ~/.cache/thumbnails/ (large / normal) の探索
    let cache_home = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = PathBuf::from(h);
                p.push(".cache");
                p
            })
        })?;

    let sizes = ["large", "normal", "x-large"];
    for size in sizes {
        let mut thumb_path = cache_home.clone();
        thumb_path.push("thumbnails");
        thumb_path.push(size);
        thumb_path.push(format!("{}.png", md5_hash));

        if thumb_path.exists() {
            if let Ok(data) = std::fs::read(&thumb_path) {
                if !data.is_empty() {
                    crate::logger::info(
                        "Thumbnail",
                        &format!("Loaded thumbnail from Freedesktop cache: {:?}", thumb_path),
                    );
                    return Some(CoverArt {
                        mime_type: "image/png".to_string(),
                        data,
                    });
                }
            }
        }
    }

    // 4. キャッシュにない場合、ffmpegthumbnailer コマンドがあれば一時ファイル経由で生成
    let temp_out = std::env::temp_dir().join(format!("myuujik_thumb_{}.png", md5_hash));
    if let Ok(status) = std::process::Command::new("ffmpegthumbnailer")
        .args([
            "-i",
            path_str,
            "-o",
            temp_out.to_str()?,
            "-s",
            "250",
        ])
        .status()
    {
        if status.success() && temp_out.exists() {
            if let Ok(data) = std::fs::read(&temp_out) {
                let _ = std::fs::remove_file(&temp_out);
                if !data.is_empty() {
                    return Some(CoverArt {
                        mime_type: "image/png".to_string(),
                        data,
                    });
                }
            }
        }
    }
    let _ = std::fs::remove_file(&temp_out);

    None
}

/// URI 文字列の MD5 ハッシュ文字列を算出
fn compute_uri_md5(uri: &str) -> Option<String> {
    // Linux 標準の md5sum コマンドを使用
    let mut child = std::process::Command::new("md5sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(uri.as_bytes());
    }

    let output = child.wait_with_output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout);
        let hash = s.split_whitespace().next()?;
        if hash.len() == 32 {
            return Some(hash.to_lowercase());
        }
    }

    None
}
