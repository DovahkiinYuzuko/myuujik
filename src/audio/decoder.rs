use std::fs::File;
use std::path::{Path, PathBuf};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecRegistry, Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS, CODEC_TYPE_PCM_F32LE, CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S32LE, CODEC_TYPE_VORBIS};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey, Tag, Value};
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;
use symphonia_adapter_libopus::OpusDecoder;

fn get_registered_codecs() -> &'static CodecRegistry {
    static REGISTRY: std::sync::OnceLock<CodecRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoder>();
        registry
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackMetadata {
    pub file_path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: Option<f64>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: Option<u32>,
    pub codec_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoverArt {
    pub mime_type: String,
    pub data: Vec<u8>,
}

pub struct AudioDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    metadata: TrackMetadata,
    cover_art: Option<CoverArt>,
    sample_buf: Option<SampleBuffer<f32>>,
}

impl AudioDecoder {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path_buf = path.as_ref().to_path_buf();
        let src = File::open(&path_buf)?;
        let mss = MediaSourceStream::new(Box::new(src), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path_buf.extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }

        let probed = get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;

        let mut format = probed.format;

        // デコード可能なオーディオトラックの探索（映像や字幕トラックを無視し、有効なオーディオを検出）
        let (track, decoder) = format
            .tracks()
            .iter()
            .find_map(|t| {
                get_registered_codecs()
                    .make(&t.codec_params, &DecoderOptions::default())
                    .ok()
                    .map(|dec| (t.clone(), dec))
            })
            .ok_or("No playable audio track found in media container")?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let bits_per_sample = track.codec_params.bits_per_sample;
        let codec_name = Self::format_codec(track.codec_params.codec);

        let duration_secs = track.codec_params.n_frames.map(|frames| {
            if let Some(tb) = track.codec_params.time_base {
                let time = tb.calc_time(frames);
                time.seconds as f64 + time.frac
            } else {
                frames as f64 / sample_rate as f64
            }
        });

        // メタデータおよびカバーアートの抽出
        let mut title = None;
        let mut artist = None;
        let mut album = None;
        let mut cover_art = None;

        if let Some(meta) = format.metadata().current() {
            Self::extract_tags(meta.tags(), &mut title, &mut artist, &mut album);
            if let Some(visual) = meta.visuals().first() {
                cover_art = Some(CoverArt {
                    mime_type: visual.media_type.clone(),
                    data: visual.data.to_vec(),
                });
            }
        }

        let metadata = TrackMetadata {
            file_path: path_buf,
            title,
            artist,
            album,
            duration_secs,
            sample_rate,
            channels,
            bits_per_sample,
            codec_name,
        };

        Ok(Self {
            format,
            decoder,
            track_id,
            metadata,
            cover_art,
            sample_buf: None,
        })
    }

    pub fn metadata(&self) -> &TrackMetadata {
        &self.metadata
    }

    pub fn cover_art(&self) -> Option<&CoverArt> {
        self.cover_art.as_ref()
    }

    /// 次のパケットをデコードし、インターリーブされた f32 サンプル列を返す。EOF時は Ok(None) を返す。
    pub fn next_interleaved_packet(&mut self) -> Result<Option<Vec<f32>>, Box<dyn std::error::Error + Send + Sync>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(ref err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(err) => return Err(err.into()),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let duration = decoded.capacity() as u64;

                    let buf = self.sample_buf.get_or_insert_with(|| {
                        SampleBuffer::<f32>::new(duration, spec)
                    });

                    if buf.capacity() < decoded.capacity() {
                        *buf = SampleBuffer::<f32>::new(duration, spec);
                    }
                    buf.copy_interleaved_ref(decoded);
                    return Ok(Some(buf.samples().to_vec()));
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    // デコードパケット破損時はスキップして次を試行
                    continue;
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// 指定された秒数位置へ高速シークを実行し、デコーダをリセットする。
    pub fn seek(&mut self, target_secs: f64) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
        let seek_time = symphonia::core::units::Time::from(target_secs);
        let actual = self.format.seek(
            SeekMode::Coarse,
            SeekTo::Time {
                time: seek_time,
                track_id: Some(self.track_id),
            },
        )?;
        self.decoder.reset();

        let actual_secs = if self.metadata.sample_rate > 0 {
            (actual.actual_ts as f64 / self.metadata.sample_rate as f64).min(target_secs.max(0.0))
        } else {
            target_secs
        };
        Ok(actual_secs.max(0.0))
    }

    fn format_codec(codec: symphonia::core::codecs::CodecType) -> String {
        match codec {
            CODEC_TYPE_AAC => "AAC".to_string(),
            CODEC_TYPE_MP3 => "MP3".to_string(),
            CODEC_TYPE_FLAC => "FLAC".to_string(),
            CODEC_TYPE_VORBIS => "Vorbis".to_string(),
            CODEC_TYPE_OPUS => "Opus".to_string(),
            CODEC_TYPE_ALAC => "ALAC".to_string(),
            CODEC_TYPE_PCM_S16LE | CODEC_TYPE_PCM_S24LE | CODEC_TYPE_PCM_S32LE | CODEC_TYPE_PCM_F32LE => "WAV/PCM".to_string(),
            _ => format!("{:?}", codec),
        }
    }

    fn extract_tags(
        tags: &[Tag],
        title: &mut Option<String>,
        artist: &mut Option<String>,
        album: &mut Option<String>,
    ) {
        for tag in tags {
            if let Some(std_key) = tag.std_key {
                match std_key {
                    StandardTagKey::TrackTitle if title.is_none() => {
                        *title = Some(Self::tag_value_to_string(&tag.value));
                    }
                    StandardTagKey::Artist if artist.is_none() => {
                        *artist = Some(Self::tag_value_to_string(&tag.value));
                    }
                    StandardTagKey::Album if album.is_none() => {
                        *album = Some(Self::tag_value_to_string(&tag.value));
                    }
                    _ => {}
                }
            }
        }
    }

    fn tag_value_to_string(value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Binary(b) => String::from_utf8_lossy(b).to_string(),
            val => val.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_open_non_existent_file_fails() {
        let result = AudioDecoder::open(Path::new("sample/non_existent_audio_file.flac"));
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_wav_sample_strictly() {
        let sample_path = Path::new("sample/Kendrick Lamar - Not Like Us.wav");
        if !sample_path.exists() {
            eprintln!("Skipping test: sample file not found");
            return;
        }

        let mut decoder = AudioDecoder::open(sample_path).expect("Failed to open WAV sample");
        let meta = decoder.metadata();
        assert!(meta.sample_rate == 44100 || meta.sample_rate == 48000 || meta.sample_rate > 0);
        assert_eq!(meta.channels, 2);
        assert!(meta.duration_secs.unwrap_or(0.0) > 0.0);
        assert_eq!(meta.codec_name, "WAV/PCM");

        // 10パケット連続デコードし、NaNやInf、範囲外サンプルがないことを検証
        let mut packets_read = 0;
        let mut total_samples = 0;

        while packets_read < 10 {
            match decoder.next_interleaved_packet() {
                Ok(Some(samples)) => {
                    assert!(!samples.is_empty(), "Decoded samples should not be empty");
                    for &sample in &samples {
                        assert!(!sample.is_nan(), "Sample should not be NaN");
                        assert!(!sample.is_infinite(), "Sample should not be Infinite");
                        assert!(sample >= -2.0 && sample <= 2.0, "Sample out of standard audio bounds: {}", sample);
                    }
                    total_samples += samples.len();
                    packets_read += 1;
                }
                Ok(None) => break,
                Err(e) => panic!("Packet decoding error: {}", e),
            }
        }
        assert!(total_samples > 0);
    }

    #[test]
    fn test_decode_mp3_sample_and_tags_strictly() {
        let sample_path = Path::new("sample/Coolio - Gangsta's Paradise (feat. L.V.) [Official Music Video].mp3");
        if !sample_path.exists() {
            eprintln!("Skipping test: sample file not found");
            return;
        }

        let mut decoder = AudioDecoder::open(sample_path).expect("Failed to open MP3 sample");
        let meta = decoder.metadata();
        assert!(meta.sample_rate > 0);
        assert!(meta.channels > 0);
        assert_eq!(meta.codec_name, "MP3");

        // パケットデコード
        let packet = decoder.next_interleaved_packet().expect("Failed to decode mp3 packet");
        assert!(packet.is_some());
        let samples = packet.unwrap();
        for &s in &samples {
            assert!(!s.is_nan());
            assert!(!s.is_infinite());
        }

        // シーク動作検証 (5秒へシーク -> 10秒へシーク -> 0秒へ巻き戻し)
        assert!(decoder.seek(5.0).is_ok());
        assert!(decoder.next_interleaved_packet().unwrap().is_some());

        assert!(decoder.seek(10.0).is_ok());
        assert!(decoder.next_interleaved_packet().unwrap().is_some());

        assert!(decoder.seek(0.0).is_ok());
        assert!(decoder.next_interleaved_packet().unwrap().is_some());
    }

    #[test]
    fn test_decode_m4a_aac_sample_strictly() {
        let sample_path = Path::new("sample/Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster).m4a");
        if !sample_path.exists() {
            eprintln!("Skipping test: sample file not found");
            return;
        }

        let mut decoder = AudioDecoder::open(sample_path).expect("Failed to open M4A sample");
        let meta = decoder.metadata();
        assert!(meta.sample_rate > 0);
        assert!(meta.channels > 0);
        assert_eq!(meta.codec_name, "AAC");

        let packet = decoder.next_interleaved_packet().expect("Failed to decode m4a packet");
        assert!(packet.is_some());
        let samples = packet.unwrap();
        for &s in &samples {
            assert!(!s.is_nan());
            assert!(!s.is_infinite());
        }
    }

    #[test]
    fn test_decode_mp4_container_sample() {
        let sample_m4a = Path::new("sample/Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster).m4a");
        if !sample_m4a.exists() {
            eprintln!("Skipping test: sample file not found");
            return;
        }

        // MP4拡張子として一時コピーしてコンテナパースをテスト
        let temp_mp4 = Path::new("sample/temp_test_rick_astley.mp4");
        std::fs::copy(sample_m4a, temp_mp4).expect("Failed to copy sample as mp4");

        let res = AudioDecoder::open(temp_mp4);
        let mut decoder = res.expect("Failed to open MP4 container");
        let meta = decoder.metadata();
        assert!(meta.sample_rate > 0);
        assert!(meta.channels > 0);
        assert_eq!(meta.codec_name, "AAC");

        // パケットデコード
        let packet = decoder.next_interleaved_packet().expect("Failed to decode mp4 packet");
        assert!(packet.is_some());

        // シーク動作検証
        assert!(decoder.seek(5.0).is_ok());
        assert!(decoder.next_interleaved_packet().unwrap().is_some());

        // テスト用一時ファイルのクリーンアップ
        let _ = std::fs::remove_file(temp_mp4);
    }

    #[test]
    fn test_decode_webm_opus_container_sample() {
        let path = Path::new("sample/sample-movie/ジャイボール.webm");
        if !path.exists() {
            eprintln!("Skipping test: sample file not found");
            return;
        }

        let mut decoder = AudioDecoder::open(path).expect("Failed to open WebM (Opus) container");
        let meta = decoder.metadata();
        assert!(meta.sample_rate > 0);
        assert_eq!(meta.codec_name, "Opus");
        // 228.181秒 (約3分48秒) であることを検証
        assert!(meta.duration_secs.is_some());
        let dur = meta.duration_secs.unwrap();
        assert!((dur - 228.181).abs() < 1.0, "Expected around 228s, got {:.3}s", dur);

        // パケットデコード
        let packet = decoder.next_interleaved_packet().expect("Failed to decode webm opus packet");
        assert!(packet.is_some());
        let samples = packet.unwrap();
        assert!(!samples.is_empty());
        for &s in &samples {
            assert!(!s.is_nan());
            assert!(!s.is_infinite());
        }

        // シーク動作検証
        assert!(decoder.seek(2.0).is_ok());
        assert!(decoder.next_interleaved_packet().unwrap().is_some());
    }
}
