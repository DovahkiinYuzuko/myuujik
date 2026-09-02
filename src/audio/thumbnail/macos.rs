use crate::audio::decoder::CoverArt;
use std::path::Path;

/// macOS QuickLook (qlmanage) を使用したサムネイル抽出
pub fn extract_thumbnail<P: AsRef<Path>>(video_path: P) -> Option<CoverArt> {
    let p = video_path.as_ref();
    let abs_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let path_str = abs_path.to_str()?;

    let temp_dir = std::env::temp_dir();
    let temp_dir_str = temp_dir.to_str()?;

    // qlmanage -t -s 250 -o <temp_dir> <file>
    let status = std::process::Command::new("qlmanage")
        .args([
            "-t",
            "-s",
            "250",
            "-o",
            temp_dir_str,
            path_str,
        ])
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // qlmanage は通常 <temp_dir>/<file_name>.png という形式で出力する
    let file_name = p.file_name()?.to_str()?;
    let expected_png = temp_dir.join(format!("{}.png", file_name));

    if expected_png.exists() {
        if let Ok(data) = std::fs::read(&expected_png) {
            let _ = std::fs::remove_file(&expected_png);
            if !data.is_empty() {
                crate::logger::info(
                    "Thumbnail",
                    &format!("Extracted thumbnail via qlmanage for {:?}", path_str),
                );
                return Some(CoverArt {
                    mime_type: "image/png".to_string(),
                    data,
                });
            }
        }
    }

    None
}
