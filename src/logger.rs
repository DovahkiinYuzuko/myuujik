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
    let total_secs = now.as_secs();
    let millis = now.subsec_millis();
    let (year, month, day, hour, min, sec) = epoch_secs_to_utc_ymd(total_secs);

    let file_ts = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        year, month, day, hour, min, sec
    );
    let log_ts = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        year, month, day, hour, min, sec, millis
    );
    (file_ts, log_ts)
}

/// UNIX エポック秒から UTC の (年, 月, 日, 時, 分, 秒) を決定論的に算出する。
pub fn epoch_secs_to_utc_ymd(total_secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (total_secs % 60) as u32;
    let total_mins = total_secs / 60;
    let min = (total_mins % 60) as u32;
    let total_hours = total_mins / 60;
    let hour = (total_hours % 24) as u32;
    let mut days = (total_hours / 24) as i64;

    let mut year = 1970i64;
    loop {
        let leap = is_leap_year(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    let leap = is_leap_year(year);
    let days_in_months = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 1u32;
    for &dim in &days_in_months {
        if days >= dim as i64 {
            days -= dim as i64;
            month += 1;
        } else {
            break;
        }
    }

    let day = (days + 1) as u32;
    (year as u32, month, day, hour, min, sec)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
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

    #[test]
    fn test_epoch_to_utc_formatting() {
        // 1970-01-01 00:00:00 UTC
        let (y, m, d, h, min, s) = epoch_secs_to_utc_ymd(0);
        assert_eq!((y, m, d, h, min, s), (1970, 1, 1, 0, 0, 0));

        // 2024-02-29 00:00:00 UTC (うるう年の検証)
        let (y, m, d, h, min, s) = epoch_secs_to_utc_ymd(1709164800);
        assert_eq!((y, m, d, h, min, s), (2024, 2, 29, 0, 0, 0));

        // 2026-09-02 04:00:00 UTC
        let (y, m, d, h, min, s) = epoch_secs_to_utc_ymd(1788321600);
        assert_eq!((y, m, d, h, min, s), (2026, 9, 2, 4, 0, 0));
    }
}

