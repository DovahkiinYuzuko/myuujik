use std::path::Path;
use chromaprint::{fingerprint_audio, Algorithm};
use super::decoder::AudioDecoder;

/// 算出された音響指紋データ
#[derive(Debug, Clone)]
pub struct AudioFingerprint {
    /// Base64エンコードされたChromaprintフィンガープリント文字列
    pub fingerprint: String,
    /// 音源全体の総再生時間（秒）
    pub duration_secs: u32,
}

/// 音源ファイルから音響指紋（Chromaprint）と再生時間を算出する
///
/// 音源の冒頭最大120秒分のPCMサンプルをデコードし、
/// AcoustID APIと完全互換のBase64フィンガープリント文字列を生成する。
pub fn calc_fingerprint<P: AsRef<Path>>(track_path: P) -> Result<AudioFingerprint, Box<dyn std::error::Error + Send + Sync>> {
    let mut decoder = AudioDecoder::open(track_path)?;
    let sample_rate = decoder.metadata().sample_rate;
    let channels = decoder.metadata().channels;
    let duration_secs = decoder.metadata().duration_secs.unwrap_or(0.0).round() as u32;

    // 音響指紋生成用の最大サンプル数（120秒分）
    let max_samples = (sample_rate as usize) * (channels as usize) * 120;
    let mut pcm_samples: Vec<i16> = Vec::with_capacity(max_samples.min(1024 * 1024));

    while let Some(packet_f32) = decoder.next_interleaved_packet()? {
        for &s in &packet_f32 {
            let s_clamped = s.clamp(-1.0, 1.0);
            let s_i16 = (s_clamped * 32767.0) as i16;
            pcm_samples.push(s_i16);
            if pcm_samples.len() >= max_samples {
                break;
            }
        }
        if pcm_samples.len() >= max_samples {
            break;
        }
    }

    if pcm_samples.is_empty() {
        return Err("No audio samples could be decoded for fingerprinting".into());
    }

    // 実際の再生秒数（メタデータから取れなかった場合のフォールバック）
    let effective_duration = if duration_secs > 0 {
        duration_secs
    } else {
        (pcm_samples.len() / (sample_rate as usize * channels as usize)) as u32
    };

    // Chromaprint アルゴリズムでフィンガープリントを計算
    let fp_result = fingerprint_audio(
        &pcm_samples,
        sample_rate,
        channels,
        Algorithm::default(),
    ).map_err(|e| format!("Failed to generate chromaprint: {e:?}"))?;

    let fingerprint = fp_result.encoded().to_string();

    Ok(AudioFingerprint {
        fingerprint,
        duration_secs: effective_duration,
    })
}
