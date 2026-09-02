use std::fs;
use std::path::Path;

/// 歌詞の1行を表す構造体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    /// 再生開始からのミリ秒
    pub timestamp_ms: u64,
    /// 歌詞テキスト
    pub text: String,
}

/// パースされた歌詞データ全体
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lyrics {
    /// 時間順にソートされた歌詞行
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    /// 指定された経過時間（ミリ秒）に最も適した現在歌唱中の行インデックスを返す
    pub fn current_line_index(&self, elapsed_ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }

        // timestamp_ms <= elapsed_ms を満たす最後の要素を二分探索
        let partition = self.lines.partition_point(|line| line.timestamp_ms <= elapsed_ms);
        if partition == 0 {
            // 最初の行のタイムスタンプ以前の場合
            Some(0)
        } else {
            Some(partition - 1)
        }
    }

    /// 歌詞の最終行のタイムスタンプ（ミリ秒）を返す。歌詞が空の場合は0を返す。
    pub fn last_timestamp_ms(&self) -> u64 {
        self.lines.last().map(|l| l.timestamp_ms).unwrap_or(0)
    }
}

/// LRC形式の文字列をパースする
pub fn parse_lrc(content: &str) -> Option<Lyrics> {
    let mut lines = Vec::new();

    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // 行内のタイムスタンプを抽出
        // 例: [00:12.34] や [00:12.345] または 複数 [00:12.34][00:24.50]歌詞
        let mut timestamps = Vec::new();
        let mut remaining = trimmed;

        while remaining.starts_with('[') {
            if let Some(close_idx) = remaining.find(']') {
                let tag = &remaining[1..close_idx];
                if let Some(ms) = parse_timestamp(tag) {
                    timestamps.push(ms);
                }
                remaining = remaining[close_idx + 1..].trim_start();
            } else {
                break;
            }
        }

        let text = remaining.trim().to_string();
        for ts in timestamps {
            lines.push(LyricLine {
                timestamp_ms: ts,
                text: text.clone(),
            });
        }
    }

    if lines.is_empty() {
        return None;
    }

    // タイムスタンプ順にソート
    lines.sort_by_key(|line| line.timestamp_ms);

    Some(Lyrics { lines })
}

/// "00:12.34" や "01:23.456" などのタイムスタンプ文字列をミリ秒に変換する
fn parse_timestamp(tag: &str) -> Option<u64> {
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes: u64 = parts[0].trim().parse().ok()?;
    let sec_parts: Vec<&str> = parts[1].split('.').collect();

    let (seconds, millis) = if sec_parts.len() == 2 {
        let secs: u64 = sec_parts[0].trim().parse().ok()?;
        let frac_str = sec_parts[1].trim();
        let ms: u64 = match frac_str.len() {
            1 => frac_str.parse::<u64>().ok()? * 100,
            2 => frac_str.parse::<u64>().ok()? * 10,
            3 => frac_str.parse::<u64>().ok()?,
            _ => frac_str[..3].parse::<u64>().ok()?,
        };
        (secs, ms)
    } else if sec_parts.len() == 1 {
        let secs: u64 = sec_parts[0].trim().parse().ok()?;
        (secs, 0)
    } else {
        return None;
    };

    Some(minutes * 60 * 1000 + seconds * 1000 + millis)
}

/// 音源ファイルのパスから同ディレクトリにある同名の `.lrc` を探索して読み込む
pub fn load_for_track(track_path: &Path) -> Option<Lyrics> {
    let lrc_path = track_path.with_extension("lrc");
    if lrc_path.is_file() {
        if let Ok(content) = fs::read_to_string(&lrc_path) {
            return parse_lrc(&content);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp() {
        assert_eq!(parse_timestamp("00:01.04"), Some(1040));
        assert_eq!(parse_timestamp("01:12.345"), Some(72345));
        assert_eq!(parse_timestamp("02:30"), Some(150000));
        assert_eq!(parse_timestamp("invalid"), None);
    }

    #[test]
    fn test_parse_lrc_basic() {
        let lrc = r#"
[00:01.04] Oh yeah
[00:08.23] If I could go back
[00:11.81] Just for a night
"#;
        let lyrics = parse_lrc(lrc).unwrap();
        assert_eq!(lyrics.lines.len(), 3);
        assert_eq!(lyrics.lines[0].text, "Oh yeah");
        assert_eq!(lyrics.lines[0].timestamp_ms, 1040);
        assert_eq!(lyrics.lines[1].timestamp_ms, 8230);
        assert_eq!(lyrics.lines[2].timestamp_ms, 11810);
    }

    #[test]
    fn test_current_line_index() {
        let lrc = r#"
[00:01.00] Line 1
[00:05.00] Line 2
[00:10.00] Line 3
"#;
        let lyrics = parse_lrc(lrc).unwrap();
        assert_eq!(lyrics.current_line_index(500), Some(0)); // 開始前
        assert_eq!(lyrics.current_line_index(1000), Some(0)); // ちょうどLine 1
        assert_eq!(lyrics.current_line_index(3000), Some(0)); // Line 1の途中
        assert_eq!(lyrics.current_line_index(5000), Some(1)); // Line 2
        assert_eq!(lyrics.current_line_index(9999), Some(1)); // Line 2の直前
        assert_eq!(lyrics.current_line_index(10000), Some(2)); // Line 3
        assert_eq!(lyrics.current_line_index(50000), Some(2)); // Line 3以降
    }
}
