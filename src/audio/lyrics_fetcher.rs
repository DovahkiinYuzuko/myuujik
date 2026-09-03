use std::fs;
use std::path::{Path, PathBuf};
use serde_json::Value;

use super::decoder::AudioDecoder;
use super::fingerprint::calc_fingerprint;
use super::lyrics::parse_lrc;
use crate::logger;

/// AcoustID 公開クライアントアプリケーションキー
const ACOUSTID_CLIENT_KEY: &str = "fgInwsQbCAw";

/// 歌詞検索結果の候補情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsCandidate {
    pub id: u64,
    pub track_name: String,
    pub artist_name: String,
    pub album_name: Option<String>,
    pub duration_secs: u32,
    pub is_synced: bool,
    pub synced_lyrics: Option<String>,
    pub plain_lyrics: Option<String>,
}

/// 音源ファイルからメタデータ（タイトル、アーティスト、秒数）を抽出する
pub fn extract_tags_from_file(path: &Path) -> (Option<String>, Option<String>, Option<u32>) {
    if let Ok(decoder) = AudioDecoder::open(path) {
        let meta = decoder.metadata();
        let dur = meta.duration_secs.map(|d| d.round() as u32);
        (meta.title.clone(), meta.artist.clone(), dur)
    } else {
        (None, None, None)
    }
}

/// ファイル名や汚れたタグからトラック番号や不要な記号・タグを除去してクリーンな検索キーワードを生成する
pub fn clean_title_from_filename(stem: &str) -> String {
    let mut s = stem.trim();

    // 先頭のトラック番号・ディスク番号プレフィックス（例: "01. ", "01 - ", "01_", "1-04. "）を除去
    loop {
        if let Some(pos) = s.find(|c: char| c == '.' || c == '-' || c == '_' || c == ' ') {
            let prefix = &s[..pos];
            if prefix.chars().all(|c| c.is_ascii_digit() || c == '-') && !prefix.is_empty() {
                let rest = s[pos + 1..].trim_start_matches(|c: char| c == '.' || c == '-' || c == '_' || c == ' ');
                if !rest.is_empty() {
                    s = rest;
                    continue;
                }
            }
        }
        break;
    }

    // 2. 括弧（(), [], 【】, 『』, 「」）に囲まれたタグを走査
    // メタタグワード（remaster, official, video, audio, lyric, mv, 4k, hd）を含む括弧は中身ごと全消去
    let tag_keywords = ["remaster", "official", "video", "audio", "lyric", "mv", "4k", "hd"];
    let bracket_pairs = [('(', ')'), ('[', ']'), ('【', '】'), ('『', '』'), ('「', '」')];

    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(&(_, close_bracket)) = bracket_pairs.iter().find(|&&(open, _)| open == c) {
            let mut inside = String::new();
            let mut found_close = false;
            for inner_c in chars.by_ref() {
                if inner_c == close_bracket {
                    found_close = true;
                    break;
                }
                inside.push(inner_c);
            }

            if found_close {
                let inside_lower = inside.to_lowercase();
                let is_meta_tag = tag_keywords.iter().any(|&kw| inside_lower.contains(kw));
                if !is_meta_tag {
                    result.push(' ');
                    result.push_str(&inside);
                    result.push(' ');
                }
            } else {
                result.push(c);
                result.push_str(&inside);
            }
        } else {
            result.push(c);
        }
    }

    // 3. 単独で浮いた "mv" トークンの除外と空白正規化
    let tokens: Vec<&str> = result
        .split_whitespace()
        .filter(|t| !t.eq_ignore_ascii_case("mv"))
        .collect();

    tokens.join(" ")
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

/// LRCLIB API (/api/get) を呼び出し、完全一致で同期歌詞を取得する
pub fn fetch_from_lrclib_exact(title: &str, artist: &str, duration_secs: u32) -> Result<String, String> {
    logger::info("LyricsFetcher", &format!("Querying LRCLIB /api/get (exact): title='{title}', artist='{artist}', duration={duration_secs}s"));

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
            logger::info("LyricsFetcher", "LRCLIB exact match returned synced lyrics");
            return Ok(synced.to_string());
        }
    }

    if let Some(plain) = json["plainLyrics"].as_str() {
        if !plain.trim().is_empty() {
            logger::info("LyricsFetcher", "LRCLIB exact match returned plain lyrics (fallback)");
            return Ok(plain.to_string());
        }
    }

    Err("LRCLIB exact match has no lyrics content".to_string())
}

/// LRCLIB API (/api/search) を呼び出し、候補一覧を取得して曲長トレランスに基づきソートして返却する
pub fn search_lrclib_candidates(
    query: &str,
    target_duration: Option<u32>,
    tolerance_secs: u32,
) -> Result<Vec<LyricsCandidate>, String> {
    logger::info("LyricsFetcher", &format!("Querying LRCLIB /api/search for candidates: query='{query}'"));

    let resp = ureq::get("https://lrclib.net/api/search")
        .set("User-Agent", "myuujik/0.1.0 (https://github.com/DovahkiinYuzuko/myuujik)")
        .query("q", query)
        .call()
        .map_err(|e| format!("LRCLIB search request failed: {e}"))?;

    let json: Value = resp.into_json()
        .map_err(|e| format!("Failed to parse LRCLIB search JSON: {e}"))?;

    let items = json.as_array()
        .ok_or_else(|| "LRCLIB search response is not an array".to_string())?;

    let mut candidates = Vec::new();

    for item in items {
        let id = item["id"].as_u64().unwrap_or(0);
        let track_name = item["trackName"].as_str().unwrap_or("Unknown").to_string();
        let artist_name = item["artistName"].as_str().unwrap_or("Unknown").to_string();
        let album_name = item["albumName"].as_str().map(|s| s.to_string());
        let duration_secs = item["duration"].as_f64().unwrap_or(0.0).round() as u32;

        let synced_lyrics = item["syncedLyrics"].as_str().filter(|s| !s.trim().is_empty()).map(|s| s.to_string());
        let plain_lyrics = item["plainLyrics"].as_str().filter(|s| !s.trim().is_empty()).map(|s| s.to_string());

        if synced_lyrics.is_none() && plain_lyrics.is_none() {
            continue;
        }

        let is_synced = synced_lyrics.is_some();

        candidates.push(LyricsCandidate {
            id,
            track_name,
            artist_name,
            album_name,
            duration_secs,
            is_synced,
            synced_lyrics,
            plain_lyrics,
        });
    }

    if let Some(target) = target_duration {
        candidates.sort_by(|a, b| {
            let diff_a = (a.duration_secs as i64 - target as i64).unsigned_abs();
            let diff_b = (b.duration_secs as i64 - target as i64).unsigned_abs();

            let within_tol_a = diff_a <= tolerance_secs as u64;
            let within_tol_b = diff_b <= tolerance_secs as u64;

            match (within_tol_a, within_tol_b) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    match (a.is_synced, b.is_synced) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => diff_a.cmp(&diff_b),
                    }
                }
            }
        });
    }

    Ok(candidates)
}

/// AcoustID API を呼び出し、フィンガープリント・秒長・ファイル名のヒントから最適な曲名・アーティスト名を取得する
pub fn lookup_acoustid(fingerprint: &str, duration_secs: u32, filename_hint: &str) -> Result<(String, String), String> {
    let url = "https://api.acoustid.org/v2/lookup";
    logger::info("LyricsFetcher", &format!("Querying AcoustID API (duration={duration_secs}s, hint='{filename_hint}')..."));

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
            logger::error("LyricsFetcher", &err_msg);
            err_msg
        })?;

    let json: Value = resp.into_json()
        .map_err(|e| {
            let err_msg = format!("Failed to parse AcoustID JSON response: {e}");
            logger::error("LyricsFetcher", &err_msg);
            err_msg
        })?;

    if json["status"].as_str() != Some("ok") {
        let msg = json["error"]["message"].as_str().unwrap_or("Unknown AcoustID error");
        let err_msg = format!("AcoustID API returned error: {msg}");
        logger::warn("LyricsFetcher", &err_msg);
        return Err(err_msg);
    }

    let results = json["results"].as_array()
        .ok_or_else(|| {
            let msg = "No results field in AcoustID response".to_string();
            logger::warn("LyricsFetcher", &msg);
            msg
        })?;

    let hint_lower = filename_hint.to_lowercase();
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

    if let Some((best_score, best_t, best_a)) = candidates.into_iter().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)) {
        let found = format!("{best_a} - {best_t}");
        logger::info("LyricsFetcher", &format!("AcoustID best candidate chosen: {found} (score={best_score:.2})"));
        return Ok((best_t, best_a));
    }

    let not_found = "No matching track or artist found for the given fingerprint".to_string();
    logger::warn("LyricsFetcher", &not_found);
    Err(not_found)
}

/// 音源ファイルから、メタデータ優先 → サニタイズ済みファイル名 → 音響指紋（AcoustID）の順で歌詞を自動取得・保存する
pub fn auto_fetch_and_save_lyrics<P: AsRef<Path>>(track_path: P, tolerance_secs: u32) -> Result<(PathBuf, String), String> {
    let path = track_path.as_ref();
    logger::info("LyricsFetcher", &format!("=== Starting lyrics auto-fetch for: {} (tolerance={}s) ===", path.display(), tolerance_secs));

    let filename_hint = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    // 音源ファイルのタグから曲名・アーティスト・秒数を優先抽出
    let (meta_title, meta_artist, meta_dur) = extract_tags_from_file(path);

    let target_duration = meta_dur.unwrap_or(0);

    let mut found_lyrics: Option<(String, String)> = None;

    // 1. Step 1: タグメタデータが存在する場合の直接検索（最優先・誤爆ゼロ）
    if let (Some(title), Some(artist)) = (&meta_title, &meta_artist) {
        let clean_t = clean_title_from_filename(title);
        let clean_a = artist.trim();
        logger::info("LyricsFetcher", &format!("Step 1 (Metadata): Checking tag metadata: '{clean_a} - {clean_t}' (dur={target_duration}s)"));

        // 1-a. LRCLIB 完全一致検索 (/api/get)
        if let Ok(lyrics) = fetch_from_lrclib_exact(&clean_t, clean_a, target_duration) {
            let coverage = calc_lyrics_coverage(&lyrics, target_duration);
            if target_duration >= 60 && coverage < 0.15 {
                logger::warn("LyricsFetcher", "Exact lyrics coverage suspiciously low, trying candidate search...");
            } else {
                found_lyrics = Some((lyrics, format!("{clean_a} - {clean_t}")));
            }
        }

        // 1-b. 完全一致で取れなかった場合はファジー候補検索
        if found_lyrics.is_none() {
            let query = format!("{clean_a} {clean_t}");
            if let Ok(candidates) = search_lrclib_candidates(&query, if target_duration > 0 { Some(target_duration) } else { None }, tolerance_secs) {
                if let Some(best) = candidates.first() {
                    let diff = (best.duration_secs as i64 - target_duration as i64).unsigned_abs();
                    if target_duration == 0 || diff <= tolerance_secs as u64 {
                        if let Some(synced) = &best.synced_lyrics {
                            found_lyrics = Some((synced.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                        } else if let Some(plain) = &best.plain_lyrics {
                            found_lyrics = Some((plain.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                        }
                    }
                }
            }
        }
    }

    // 2. Step 2: タグで取れなかった場合のファイル名検索（サニタイズ済み）
    if found_lyrics.is_none() {
        let cleaned_query = clean_title_from_filename(filename_hint);
        logger::info("LyricsFetcher", &format!("Step 2 (Filename): Trying cleaned filename query: '{cleaned_query}'"));

        if let Ok(candidates) = search_lrclib_candidates(&cleaned_query, if target_duration > 0 { Some(target_duration) } else { None }, tolerance_secs) {
            if let Some(best) = candidates.first() {
                let diff = (best.duration_secs as i64 - target_duration as i64).unsigned_abs();
                if target_duration == 0 || diff <= tolerance_secs as u64 {
                    if let Some(synced) = &best.synced_lyrics {
                        found_lyrics = Some((synced.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                    } else if let Some(plain) = &best.plain_lyrics {
                        found_lyrics = Some((plain.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                    }
                }
            }
        }
    }

    // 3. Step 3: 音響指紋（AcoustID）フォールバック（最後の手段）
    if found_lyrics.is_none() {
        logger::info("LyricsFetcher", "Step 3 (AcoustID): Calculating fingerprint as fallback...");
        if let Ok(fp) = calc_fingerprint(path) {
            let fp_dur = if target_duration > 0 { target_duration } else { fp.duration_secs };
            if let Ok((title, artist)) = lookup_acoustid(&fp.fingerprint, fp_dur, filename_hint) {
                if let Ok(candidates) = search_lrclib_candidates(&format!("{artist} {title}"), Some(fp_dur), tolerance_secs) {
                    if let Some(best) = candidates.first() {
                        let diff = (best.duration_secs as i64 - fp_dur as i64).unsigned_abs();
                        if diff <= tolerance_secs as u64 {
                            if let Some(synced) = &best.synced_lyrics {
                                found_lyrics = Some((synced.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                            } else if let Some(plain) = &best.plain_lyrics {
                                found_lyrics = Some((plain.clone(), format!("{} - {}", best.artist_name, best.track_name)));
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. 歌詞の保存
    if let Some((lyrics_content, track_display)) = found_lyrics {
        let lrc_path = path.with_extension("lrc");
        fs::write(&lrc_path, &lyrics_content)
            .map_err(|e| {
                let err_msg = format!("Failed to write LRC file to {}: {e}", lrc_path.display());
                logger::error("LyricsFetcher", &err_msg);
                err_msg
            })?;

        logger::info("LyricsFetcher", &format!("Successfully saved lyrics to: {}", lrc_path.display()));
        Ok((lrc_path, track_display))
    } else {
        let err_msg = "No matching lyrics found within duration tolerance".to_string();
        logger::warn("LyricsFetcher", &err_msg);
        Err(err_msg)
    }
}
