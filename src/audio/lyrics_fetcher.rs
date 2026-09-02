use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::Value;

use super::fingerprint::calc_fingerprint;
use super::lyrics::parse_lrc;

/// AcoustID 公開クライアントアプリケーションキー
const ACOUSTID_CLIENT_KEY: &str = "fgInwsQbCAw";

/// プロジェクトルートの myuujik.log にログをタイムスタンプ付きで記録する
pub fn log_event(level: &str, message: &str) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let log_line = format!("[{now}s] [{level}] {message}\n");

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("myuujik.log") {
        let _ = file.write_all(log_line.as_bytes());
    }
}

/// YouTube動画タイトル等から不要な記号やタグを除去してクリーンな検索キーワードを生成する
pub fn clean_title_from_filename(stem: &str) -> String {
    let mut cleaned = stem.to_string();

    let tags_to_remove = [
        "(HD)", "(4K)", "(Official Video)", "(Official Music Video)",
        "[Official Music Video]", "(4K Remaster)", "[Official Video]",
        "(Audio)", "[Audio]", "(Lyric Video)", "[Lyric Video]", "(MV)", "[MV]",
    ];

    for tag in tags_to_remove {
        cleaned = cleaned.replace(tag, " ");
    }

    let bracket_chars = ['『', '』', '「', '」', '【', '】', '[', ']', '(', ')'];
    for ch in bracket_chars {
        cleaned = cleaned.replace(ch, " ");
    }

    cleaned.split_whitespace().collect::<Vec<&str>>().join(" ")
}

/// 歌詞の最終タイムスタンプと曲長から、歌詞が曲のどれくらい終盤までカバーしているか（網羅率: 0.0〜1.0）を算出する
pub fn calc_lyrics_coverage(synced_lrc: &str, track_duration_secs: u32) -> f64 {
    if track_duration_secs == 0 {
        return 1.0;
    }
    if let Some(lyrics) = parse_lrc(synced_lrc) {
        let last_ms = lyrics.last_timestamp_ms();
        let last_secs = last_ms as f64 / 1000.0;
        (last_secs / track_duration_secs as f64).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// AcoustID API を呼び出し、フィンガープリント・秒長・ファイル名のヒントから最適な曲名・アーティスト名を取得する
pub fn lookup_acoustid(fingerprint: &str, duration_secs: u32, filename_hint: &str) -> Result<(String, String), String> {
    let url = "https://api.acoustid.org/v2/lookup";
    log_event("INFO", &format!("Querying AcoustID API (duration={duration_secs}s, hint='{filename_hint}')..."));

    let resp = ureq::post(url)
        .set("User-Agent", "myuujik/0.1.0 (https://github.com/DovahkiinYuzuko/myuujik)")
        .send_form(&[
            ("client", ACOUSTID_CLIENT_KEY),
            ("meta", "recordings releasegroups compress"),
            ("duration", &duration_secs.to_string()),
            ("fingerprint", fingerprint),
        ])
        .map_err(|e| {
            let err_msg = format!("AcoustID HTTP request failed: {e}");
            log_event("ERROR", &err_msg);
            err_msg
        })?;

    let json: Value = resp.into_json()
        .map_err(|e| {
            let err_msg = format!("Failed to parse AcoustID JSON response: {e}");
            log_event("ERROR", &err_msg);
            err_msg
        })?;

    if json["status"].as_str() != Some("ok") {
        let msg = json["error"]["message"].as_str().unwrap_or("Unknown AcoustID error");
        let err_msg = format!("AcoustID API returned error: {msg}");
        log_event("WARN", &err_msg);
        return Err(err_msg);
    }

    let results = json["results"].as_array()
        .ok_or_else(|| {
            let msg = "No results field in AcoustID response".to_string();
            log_event("WARN", &msg);
            msg
        })?;

    let hint_lower = filename_hint.to_lowercase();

    // 候補の中から (確信度スコア + ファイル名一致ボーナス) が最大のものを選択
    let mut candidates: Vec<(f64, String, String)> = Vec::new();

    for res in results {
        let base_score = res["score"].as_f64().unwrap_or(0.5);

        if let Some(recordings) = res["recordings"].as_array() {
            for rec in recordings {
                let title = rec["title"].as_str();
                let artist = rec["artists"].as_array()
                    .and_then(|artists| artists.first())
                    .and_then(|a| a["name"].as_str());

                if let (Some(t), Some(a)) = (title, artist) {
                    let mut match_score = base_score;

                    // ファイル名に曲名またはアーティスト名が含まれている場合は大幅に加点
                    let t_lower = t.to_lowercase();
                    let a_lower = a.to_lowercase();
                    if hint_lower.contains(&t_lower) {
                        match_score += 1.0;
                    }
                    if hint_lower.contains(&a_lower) {
                        match_score += 0.5;
                    }

                    candidates.push((match_score, t.to_string(), a.to_string()));
                }
            }
        }
    }

    // 最高スコアの候補を採用
    if let Some((best_score, best_t, best_a)) = candidates.into_iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)) {
        let found = format!("{best_a} - {best_t}");
        log_event("INFO", &format!("AcoustID best candidate chosen: {found} (score={best_score:.2})"));
        return Ok((best_t, best_a));
    }

    let not_found = "No matching track or artist found for the given fingerprint".to_string();
    log_event("WARN", &not_found);
    Err(not_found)
}

/// LRCLIB API (/api/get) を呼び出し、完全一致で同期歌詞を取得する
pub fn fetch_from_lrclib_exact(title: &str, artist: &str, duration_secs: u32) -> Result<String, String> {
    log_event("INFO", &format!("Querying LRCLIB /api/get (exact): title='{title}', artist='{artist}'"));

    let mut req = ureq::get("https://lrclib.net/api/get")
        .set("User-Agent", "myuujik/0.1.0 (https://github.com/DovahkiinYuzuko/myuujik)")
        .query("track_name", title)
        .query("artist_name", artist);

    if duration_secs > 0 {
        req = req.query("duration", &duration_secs.to_string());
    }

    let resp = req.call()
        .map_err(|e| format!("LRCLIB exact request failed: {e}"))?;

    let json: Value = resp.into_json()
        .map_err(|e| format!("Failed to parse LRCLIB exact JSON: {e}"))?;

    if let Some(synced) = json["syncedLyrics"].as_str() {
        if !synced.trim().is_empty() {
            log_event("INFO", "LRCLIB exact match returned synced lyrics");
            return Ok(synced.to_string());
        }
    }

    if let Some(plain) = json["plainLyrics"].as_str() {
        if !plain.trim().is_empty() {
            log_event("INFO", "LRCLIB exact match returned plain lyrics (fallback)");
            return Ok(plain.to_string());
        }
    }

    Err("LRCLIB exact match has no lyrics content".to_string())
}

/// LRCLIB API (/api/search) を呼び出し、カバレッジと曲長から最も健全な同期歌詞を取得する
pub fn search_lrclib_fuzzy(query: &str, target_duration: u32) -> Result<(String, String), String> {
    log_event("INFO", &format!("Querying LRCLIB /api/search (fuzzy): query='{query}'"));

    let resp = ureq::get("https://lrclib.net/api/search")
        .set("User-Agent", "myuujik/0.1.0 (https://github.com/DovahkiinYuzuko/myuujik)")
        .query("q", query)
        .call()
        .map_err(|e| format!("LRCLIB search request failed: {e}"))?;

    let json: Value = resp.into_json()
        .map_err(|e| format!("Failed to parse LRCLIB search JSON: {e}"))?;

    let items = json.as_array()
        .ok_or_else(|| "LRCLIB search response is not an array".to_string())?;

    if items.is_empty() {
        return Err("LRCLIB fuzzy search returned zero results".to_string());
    }

    // 各候補のスコアリング: (健全度スコア, syncedLyrics, track_display)
    // 健全度 = (カバレッジ率 * 100.0) - (曲長誤差 * 1.0)
    let mut scored_candidates: Vec<(f64, String, String)> = Vec::new();

    for item in items {
        if let Some(synced) = item["syncedLyrics"].as_str() {
            if !synced.trim().is_empty() {
                let item_dur = item["duration"].as_f64().unwrap_or(0.0).round() as u32;
                let diff = (item_dur as i64 - target_duration as i64).unsigned_abs() as f64;
                let coverage = calc_lyrics_coverage(synced, target_duration);

                // 曲長カバレッジが健全（終盤まである）候補を高く評価
                let score = (coverage * 100.0) - (diff * 0.5);

                let t = item["trackName"].as_str().unwrap_or("Unknown");
                let a = item["artistName"].as_str().unwrap_or("Unknown");
                let track_display = format!("{a} - {t}");

                scored_candidates.push((score, synced.to_string(), track_display));
            }
        }
    }

    // 最もスコアの高い候補を採用
    if let Some((score, synced, display)) = scored_candidates.into_iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)) {
        log_event("INFO", &format!("LRCLIB fuzzy best candidate chosen: {display} (score={score:.1})"));
        return Ok((synced, display));
    }

    // 同期歌詞がなければ平文歌詞でフォールバック
    for item in items {
        if let Some(plain) = item["plainLyrics"].as_str() {
            if !plain.trim().is_empty() {
                let t = item["trackName"].as_str().unwrap_or("Unknown");
                let a = item["artistName"].as_str().unwrap_or("Unknown");
                let track_display = format!("{a} - {t}");
                log_event("INFO", &format!("LRCLIB fuzzy match found plain lyrics: {track_display}"));
                return Ok((plain.to_string(), track_display));
            }
        }
    }

    Err("No valid lyrics found in LRCLIB fuzzy search results".to_string())
}

/// 音源ファイルから音響指紋およびファイル名フォールバックを用いて歌詞を自動取得・保存する
pub fn auto_fetch_and_save_lyrics<P: AsRef<Path>>(track_path: P) -> Result<(PathBuf, String), String> {
    let path = track_path.as_ref();
    log_event("INFO", &format!("=== Starting lyrics auto-fetch for: {} ===", path.display()));

    let filename_hint = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    // 1. 音響指紋の算出
    let fp = calc_fingerprint(path)
        .map_err(|e| {
            let err_msg = format!("Fingerprint error: {e}");
            log_event("ERROR", &err_msg);
            err_msg
        })?;
    log_event("INFO", &format!("Fingerprint generated: duration={}s", fp.duration_secs));

    let mut found_lyrics: Option<(String, String)> = None;

    // 2. Step 1: AcoustID API で楽曲特定を試行（ファイル名ヒント付きスコアリング）
    if let Ok((title, artist)) = lookup_acoustid(&fp.fingerprint, fp.duration_secs, filename_hint) {
        let mut try_fuzzy = false;

        // 2-a. LRCLIB 完全一致検索
        if let Ok(lyrics) = fetch_from_lrclib_exact(&title, &artist, fp.duration_secs) {
            let coverage = calc_lyrics_coverage(&lyrics, fp.duration_secs);
            // 曲長が60秒以上あるのにカバレッジが15%未満（リックロール等のジョークデータ）の場合はファジー検索でフル版を探す
            if fp.duration_secs >= 60 && coverage < 0.15 {
                log_event("WARN", &format!("Exact match lyrics coverage is suspiciously low ({:.1}%). Trying fuzzy search for complete lyrics...", coverage * 100.0));
                try_fuzzy = true;
            } else {
                found_lyrics = Some((lyrics, format!("{artist} - {title}")));
            }
        } else {
            try_fuzzy = true;
        }

        // 2-b. LRCLIB ファジー検索
        if try_fuzzy {
            let query = format!("{artist} {title}");
            if let Ok((lyrics, display)) = search_lrclib_fuzzy(&query, fp.duration_secs) {
                found_lyrics = Some((lyrics, display));
            }
        }
    }

    // 3. Step 2: AcoustID でヒットしなかった場合のファイル名フォールバック
    if found_lyrics.is_none() {
        let cleaned_query = clean_title_from_filename(filename_hint);
        log_event("INFO", &format!("AcoustID lookup missed. Trying filename fallback with query: '{cleaned_query}'"));
        if let Ok((lyrics, display)) = search_lrclib_fuzzy(&cleaned_query, fp.duration_secs) {
            found_lyrics = Some((lyrics, display));
        }
    }

    // 4. 歌詞の保存
    if let Some((lyrics_content, track_display)) = found_lyrics {
        let lrc_path = path.with_extension("lrc");
        fs::write(&lrc_path, &lyrics_content)
            .map_err(|e| {
                let err_msg = format!("Failed to write LRC file to {}: {e}", lrc_path.display());
                log_event("ERROR", &err_msg);
                err_msg
            })?;

        log_event("INFO", &format!("Successfully saved lyrics to: {}", lrc_path.display()));
        Ok((lrc_path, track_display))
    } else {
        let err_msg = "No lyrics found via AcoustID or filename search".to_string();
        log_event("WARN", &err_msg);
        Err(err_msg)
    }
}
