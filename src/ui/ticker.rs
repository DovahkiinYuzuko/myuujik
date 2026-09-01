#[derive(Debug, Clone)]
pub struct MarqueeTicker {
    pub initial_wait_ms: u128,
    pub step_interval_ms: u128,
    pub separator: String,
}

impl Default for MarqueeTicker {
    fn default() -> Self {
        Self {
            initial_wait_ms: 1500,
            step_interval_ms: 200,
            separator: "   +++   ".to_string(),
        }
    }
}

impl MarqueeTicker {
    pub fn new(initial_wait_ms: u128, step_interval_ms: u128, separator: &str) -> Self {
        Self {
            initial_wait_ms,
            step_interval_ms,
            separator: separator.to_string(),
        }
    }

    /// 対象文字列を表示枠幅および経過時間に合わせてスライス・整形した文字列を生成
    pub fn render(&self, text: &str, max_cells: usize, elapsed_ms: u128) -> String {
        let total_w = str_width(text);
        if total_w <= max_cells {
            return text.to_string();
        }

        // 初期ウェイト中（1.5秒）は冒頭からmax_cells分を切り出して静止
        if elapsed_ms < self.initial_wait_ms {
            return take_cells(text, max_cells);
        }

        // スクロールステップ数
        let scroll_time = elapsed_ms - self.initial_wait_ms;
        let step = (scroll_time / self.step_interval_ms) as usize;

        // ループ文字列: text + separator
        let loop_text = format!("{}{}", text, self.separator);
        let loop_w = str_width(&loop_text);

        if loop_w == 0 {
            return text.to_string();
        }

        let offset = step % loop_w;

        // 2周分連結してスライスを安全に取得
        let double_text = format!("{}{}", loop_text, loop_text);
        slice_cells(&double_text, offset, max_cells)
    }
}

/// 文字列のセル幅を計算
pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

pub fn char_width(c: char) -> usize {
    if c.is_ascii() {
        if c.is_ascii_control() {
            0
        } else {
            1
        }
    } else {
        match c as u32 {
            0x0000..=0x001F | 0x007F..=0x009F => 0,
            0x3000..=0x303F
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0xF900..=0xFAFF
            | 0xAC00..=0xD7AF => 2,
            _ => 1,
        }
    }
}

/// 先頭から max_cells に収まる部分文字列を取得
pub fn take_cells(s: &str, max_cells: usize) -> String {
    let mut current_cells = 0;
    let mut result = String::new();
    for c in s.chars() {
        let w = char_width(c);
        if current_cells + w > max_cells {
            break;
        }
        current_cells += w;
        result.push(c);
    }
    result
}

/// start_cell から max_cells 分の文字列を切り出し（セル境界で文字が跨ぐ場合は空白でパディング）
pub fn slice_cells(s: &str, start_cell: usize, max_cells: usize) -> String {
    let mut current_cell = 0;
    let mut result = String::new();
    let mut result_cells = 0;

    for c in s.chars() {
        let w = char_width(c);
        let next_cell = current_cell + w;

        if next_cell <= start_cell {
            current_cell = next_cell;
            continue;
        }

        if current_cell < start_cell {
            result.push(' ');
            result_cells += 1;
            current_cell = next_cell;
            continue;
        }

        if result_cells + w > max_cells {
            if result_cells < max_cells {
                result.push(' ');
            }
            break;
        }

        result.push(c);
        result_cells += w;
        current_cell = next_cell;

        if result_cells >= max_cells {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_char_and_str_width() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('1'), 1);
        assert_eq!(char_width(' '), 1);
        assert_eq!(char_width('あ'), 2);
        assert_eq!(char_width('漢'), 2);
        assert_eq!(char_width('🎵'), 1);

        assert_eq!(str_width("Hello"), 5);
        assert_eq!(str_width("こんにちは"), 10);
        assert_eq!(str_width("Hello こんにちは"), 16);
    }

    #[test]
    fn test_marquee_short_text() {
        let ticker = MarqueeTicker::default();
        let rendered = ticker.render("Short Song", 20, 5000);
        assert_eq!(rendered, "Short Song");
    }

    #[test]
    fn test_marquee_initial_wait() {
        let ticker = MarqueeTicker::default();
        let long_title = "Very Long Track Name That Needs Scrolling";
        // 1500ms未満は先頭から切り出し静止
        let wait_render = ticker.render(long_title, 10, 500);
        assert_eq!(wait_render, "Very Long ");
    }

    #[test]
    fn test_marquee_scrolling() {
        let ticker = MarqueeTicker::new(1000, 100, " + ");
        let title = "ABCDEFGHIJ"; // 幅10
        // 1000ms: 開始静止 "ABCDE" (幅5)
        assert_eq!(ticker.render(title, 5, 1000), "ABCDE");
        // 1100ms: 1セル進む "BCDEF"
        assert_eq!(ticker.render(title, 5, 1100), "BCDEF");
        // 1200ms: 2セル進む "CDEFG"
        assert_eq!(ticker.render(title, 5, 1200), "CDEFG");
    }
}
