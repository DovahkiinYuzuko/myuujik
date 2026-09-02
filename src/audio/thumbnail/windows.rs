use crate::audio::decoder::CoverArt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
};

/// Windows Shell API (IShellItemImageFactory) を使用して動画のサムネイルを取得する
pub fn extract_thumbnail<P: AsRef<Path>>(video_path: P) -> Option<CoverArt> {
    let p = video_path.as_ref();
    let abs_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let path_str = abs_path.to_str()?;

    // パスをワイド文字列（UTF-16, null終端）に変換
    // \\?\ プレフィックスが付いていると Shell API でエラーになる場合があるため除去
    let clean_path = if path_str.starts_with(r"\\?\") {
        &path_str[4..]
    } else {
        path_str
    };

    let wide_path: Vec<u16> = std::ffi::OsStr::new(clean_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // COM 初期化 (STA)（すでに初期化済みでもエラーは無視可能）
    let com_inited = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };

    let result = (|| -> Option<CoverArt> {
        let factory: IShellItemImageFactory = match unsafe {
            SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None)
        } {
            Ok(f) => f,
            Err(e) => {
                crate::logger::warn(
                    "Thumbnail",
                    &format!("SHCreateItemFromParsingName failed for {:?}: {:?}", clean_path, e),
                );
                return None;
            }
        };

        let size = SIZE { cx: 250, cy: 250 };
        let hbitmap = match unsafe { factory.GetImage(size, SIIGBF_RESIZETOFIT) } {
            Ok(h) => h,
            Err(e) => {
                crate::logger::warn(
                    "Thumbnail",
                    &format!("factory.GetImage failed for {:?}: {:?}", clean_path, e),
                );
                return None;
            }
        };

        // HBITMAP からサイズおよびピクセルデータを取得
        let mut bm = BITMAP::default();
        let get_obj_res = unsafe {
            GetObjectW(
                hbitmap,
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut std::ffi::c_void),
            )
        };

        if get_obj_res == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
            unsafe { let _ = DeleteObject(hbitmap); }
            return None;
        }

        let width = bm.bmWidth as usize;
        let height = bm.bmHeight.unsigned_abs() as usize;

        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bm.bmWidth,
                biHeight: -(height as i32), // Top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut raw_pixels = vec![0u8; width * height * 4];
        let screen_dc = unsafe { GetDC(HWND(std::ptr::null_mut())) };
        let lines_copied = unsafe {
            GetDIBits(
                screen_dc,
                hbitmap,
                0,
                height as u32,
                Some(raw_pixels.as_mut_ptr() as *mut std::ffi::c_void),
                &mut bi,
                DIB_RGB_COLORS,
            )
        };

        // HDC と HBITMAP の破棄
        unsafe {
            let _ = ReleaseDC(HWND(std::ptr::null_mut()), screen_dc);
            let _ = DeleteObject(hbitmap);
        }

        if lines_copied == 0 {
            crate::logger::warn("Thumbnail", &format!("GetDIBits copied 0 lines for {:?}", clean_path));
            return None;
        }

        // BGRA から RGB 3チャンネルへ変換（JPEG保存用）
        let mut rgb_pixels = Vec::with_capacity(width * height * 3);
        for chunk in raw_pixels.chunks_exact(4) {
            let b = chunk[0];
            let g = chunk[1];
            let r = chunk[2];
            rgb_pixels.extend_from_slice(&[r, g, b]);
        }

        let img = match image::RgbImage::from_raw(width as u32, height as u32, rgb_pixels) {
            Some(i) => i,
            None => {
                crate::logger::warn("Thumbnail", "RgbImage::from_raw failed");
                return None;
            }
        };

        let mut jpeg_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Jpeg) {
            crate::logger::warn("Thumbnail", &format!("write_to Jpeg failed: {:?}", e));
            return None;
        }

        crate::logger::info(
            "Thumbnail",
            &format!(
                "Successfully extracted thumbnail via IShellItemImageFactory: {:?} ({}x{}, size={} bytes)",
                clean_path, width, height, jpeg_bytes.len()
            ),
        );

        Some(CoverArt {
            mime_type: "image/jpeg".to_string(),
            data: jpeg_bytes,
        })
    })();

    if com_inited {
        unsafe { CoUninitialize(); }
    }

    result
}
