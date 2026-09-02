use crate::audio::decoder::CoverArt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::{
    IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT,
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

    // COM 初期化（すでに初期化済みでもエラーは無視可能）
    let com_inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };

    let result = (|| -> Option<CoverArt> {
        let shell_item: IShellItem = unsafe {
            SHCreateItemFromParsingName(PCWSTR(wide_path.as_ptr()), None).ok()?
        };

        let factory: IShellItemImageFactory = shell_item.cast().ok()?;

        let size = SIZE { cx: 250, cy: 250 };
        let hbitmap = unsafe {
            factory.GetImage(size, SIIGBF_RESIZETOFIT).ok()?
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
        let lines_copied = unsafe {
            GetDIBits(
                HDC(std::ptr::null_mut()),
                hbitmap,
                0,
                height as u32,
                Some(raw_pixels.as_mut_ptr() as *mut std::ffi::c_void),
                &mut bi,
                DIB_RGB_COLORS,
            )
        };

        // HBITMAP の破棄
        unsafe { let _ = DeleteObject(hbitmap); }

        if lines_copied == 0 {
            return None;
        }

        // BGRA から RGBA へ変換（アルファ値がすべて0の場合は255に補正）
        let mut rgba_pixels = Vec::with_capacity(width * height * 4);
        let mut has_non_zero_alpha = false;

        for chunk in raw_pixels.chunks_exact(4) {
            if chunk[3] > 0 {
                has_non_zero_alpha = true;
                break;
            }
        }

        for chunk in raw_pixels.chunks_exact(4) {
            let b = chunk[0];
            let g = chunk[1];
            let r = chunk[2];
            let a = if has_non_zero_alpha { chunk[3] } else { 255 };
            rgba_pixels.extend_from_slice(&[r, g, b, a]);
        }

        let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba_pixels)?;

        let mut jpeg_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);
        img.write_to(&mut cursor, image::ImageFormat::Jpeg).ok()?;

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
