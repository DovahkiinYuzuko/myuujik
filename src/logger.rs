use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

const MAX_LOG_FILES: usize = 10;
static LOGGER_INSTANCE: Mutex<Option<FileLogger>> = Mutex::new(None);

pub struct FileLogger {
    file: File,
    log_dir: PathBuf,
}

impl FileLogger {
    pub fn init<P: AsRef<Path>>(log_dir: P) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let dir = log_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        // 直近10件を超えた古いログファイルを自動削除
        Self::rotate_logs(&dir, MAX_LOG_FILES - 1)?;

        let (file_ts, _) = get_local_timestamp();
        let filename = format!("myuujik_{}.log", file_ts);
        let log_path = dir.join(filename);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;

        let mut logger = LOGGER_INSTANCE.lock().unwrap();
        *logger = Some(FileLogger { file, log_dir: dir });

        Ok(())
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    pub fn log(level: &str, module: &str, message: &str) {
        if let Ok(mut guard) = LOGGER_INSTANCE.lock() {
            if let Some(logger) = guard.as_mut() {
                let (_, log_ts) = get_local_timestamp();

                let line = format!(
                    "[{}] [{}] [{}] {}\n",
                    log_ts, level, module, message
                );
                let _ = logger.file.write_all(line.as_bytes());
                let _ = logger.file.flush();
            }
        }
    }

    fn rotate_logs(dir: &Path, keep_count: usize) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !dir.exists() {
            return Ok(());
        }

        let mut log_files = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("log") {
                if let Ok(meta) = entry.metadata() {
                    let modified = meta.modified().unwrap_or(UNIX_EPOCH);
                    log_files.push((path, modified));
                }
            }
        }

        // 更新日時が古い順（昇順）にソート
        log_files.sort_by_key(|(_, modified)| *modified);

        // keep_count を超える古いログを削除
        if log_files.len() > keep_count {
            let remove_count = log_files.len() - keep_count;
            for (path, _) in log_files.iter().take(remove_count) {
                let _ = fs::remove_file(path);
            }
        }

        Ok(())
    }
}

#[cfg(windows)]
pub fn get_local_timestamp() -> (String, String) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    let file_ts = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    );
    let log_ts = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    );
    (file_ts, log_ts)
}

#[cfg(not(windows))]
pub fn get_local_timestamp() -> (String, String) {
    let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let file_ts = format!("{}_{:03}", secs, millis);
    let log_ts = format!("{}.{:03}", secs, millis);
    (file_ts, log_ts)
}

pub fn info(module: &str, message: &str) {
    FileLogger::log("INFO", module, message);
}

pub fn warn(module: &str, message: &str) {
    FileLogger::log("WARN", module, message);
}

pub fn error(module: &str, message: &str) {
    FileLogger::log("ERROR", module, message);
}

pub fn debug(module: &str, message: &str) {
    FileLogger::log("DEBUG", module, message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_logger_rotation_and_writing() {
        let test_dir = Path::new("target/test_logs");
        let _ = fs::remove_dir_all(test_dir);
        fs::create_dir_all(test_dir).unwrap();

        // 12個のダミーログファイルを作成
        for i in 0..12 {
            let f = test_dir.join(format!("dummy_{:02}.log", i));
            fs::write(&f, "test").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        // ローテーション実行（直近10件を保持）
        FileLogger::rotate_logs(test_dir, 10).unwrap();

        let count = fs::read_dir(test_dir).unwrap().count();
        assert_eq!(count, 10, "Should rotate to exactly 10 log files");

        // ロガー初期化と書き込みテスト
        FileLogger::init(test_dir).unwrap();
        info("TestModule", &format!("Hello test log: {}", 42));
        error("TestModule", &format!("Error test: {}", "sample"));

        let _ = fs::remove_dir_all(test_dir);
    }
}
