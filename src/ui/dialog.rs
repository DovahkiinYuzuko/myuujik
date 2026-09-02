use std::path::PathBuf;

/// OS標準のフォルダ選択ダイアログを開き、ユーザーが選択したディレクトリのパスを返す
pub fn pick_folder() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        pick_folder_windows()
    }
    #[cfg(target_os = "macos")]
    {
        pick_folder_macos()
    }
    #[cfg(target_os = "linux")]
    {
        pick_folder_linux()
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(windows)]
fn pick_folder_windows() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::Foundation::HWND;

    unsafe {
        let com_inited = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();

        let result = (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;

            let mut options = dialog.GetOptions().ok()?;
            options |= FOS_PICKFOLDERS;
            dialog.SetOptions(options).ok()?;

            // 親ウィンドウなし（デスクトップ）でダイアログをモーダル表示
            dialog.Show(HWND(std::ptr::null_mut())).ok()?;

            let item = dialog.GetResult().ok()?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path_str = name.to_string().ok()?;
            CoTaskMemFree(Some(name.0 as _));

            if path_str.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(path_str))
            }
        })();

        if com_inited {
            CoUninitialize();
        }

        result
    }
}

#[cfg(target_os = "macos")]
fn pick_folder_macos() -> Option<PathBuf> {
    let script = r#"POSIX path of (choose folder with prompt "Select Music Folder")"#;
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn pick_folder_linux() -> Option<PathBuf> {
    // 1. Zenity (GNOME / デスクトップ汎用)
    if let Ok(output) = std::process::Command::new("zenity")
        .args(["--file-selection", "--directory", "--title=Select Music Folder"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }

    // 2. KDialog (KDE)
    if let Ok(output) = std::process::Command::new("kdialog")
        .args(["--getexistingdirectory", "--title", "Select Music Folder"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }

    None
}
