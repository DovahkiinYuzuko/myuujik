use symphonia::core::audio::Channels;
use symphonia::core::codecs::{CodecParameters, CodecRegistry, Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_OPUS, CODEC_TYPE_VORBIS};
use symphonia::core::formats::Track;

/// トラックネゴシエーションの状態遷移
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationState {
    /// メディアコンテナのトラック情報を取得中
    Probing,
    /// 指定インデックスのトラックを検証中
    InspectingTrack { index: usize, total: usize },
    /// コーデックパラメータを規格準拠で正規化中
    SanitizingParams { track_id: u32 },
    /// デコーダインスタンスの初期化を試行中
    InstantiatingDecoder { track_id: u32 },
    /// 有効なオーディオトラックとデコーダの確立に成功
    Ready { track_id: u32 },
    /// すべての候補トラックでデコーダ確立に失敗
    Failed { reason: String },
}

/// トラック選定およびコーデックパラメータ正規化を司る有限状態機械
pub struct TrackNegotiationFsm {
    state: NegotiationState,
}

impl TrackNegotiationFsm {
    pub fn new() -> Self {
        Self {
            state: NegotiationState::Probing,
        }
    }

    pub fn state(&self) -> &NegotiationState {
        &self.state
    }

    /// RFC 7845（OpusHead）およびコンテナ仕様に準拠したコーデックパラメータの正規化
    pub fn sanitize_codec_parameters(params: &CodecParameters) -> CodecParameters {
        let mut sanitized = params.clone();

        // 1. Opus コーデックのパラメータ補正 (MP4 / ISOBMFF 等で channels が抜ける問題の解決)
        if sanitized.codec == CODEC_TYPE_OPUS {
            if sanitized.channels.is_none() {
                if let Some(ref extra) = sanitized.extra_data {
                    if extra.starts_with(b"OpusHead") && extra.len() >= 10 {
                        let ch_count = extra[9];
                        let channels = match ch_count {
                            1 => Channels::FRONT_CENTRE,
                            2 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
                            3 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT | Channels::FRONT_CENTRE,
                            4 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT | Channels::REAR_LEFT | Channels::REAR_RIGHT,
                            5 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT | Channels::FRONT_CENTRE | Channels::REAR_LEFT | Channels::REAR_RIGHT,
                            6 => Channels::FRONT_LEFT | Channels::FRONT_RIGHT | Channels::FRONT_CENTRE | Channels::LFE1 | Channels::REAR_LEFT | Channels::REAR_RIGHT,
                            _ => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
                        };
                        sanitized.channels = Some(channels);
                        crate::logger::info("TrackNegotiationFsm", &format!("RFC 7845 OpusHead sanitized: channel_count={}", ch_count));
                    }
                }
            }

            // サンプルレートが未指定の場合は Opus 内部標準の 48,000Hz を設定
            if sanitized.sample_rate.is_none() {
                sanitized.sample_rate = Some(48000);
            }
        }

        // 2. AAC / Vorbis 等のフェイルセーフ補完
        if sanitized.codec == CODEC_TYPE_AAC || sanitized.codec == CODEC_TYPE_VORBIS {
            if sanitized.channels.is_none() {
                sanitized.channels = Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT);
            }
            if sanitized.sample_rate.is_none() {
                sanitized.sample_rate = Some(44100);
            }
        }

        sanitized
    }

    /// コンテナ内の全トラックを走査し、最適なオーディオトラックとデコーダをネゴシエーション・確立する
    pub fn negotiate(
        &mut self,
        tracks: &[Track],
        registry: &CodecRegistry,
        options: &DecoderOptions,
    ) -> Result<(Track, Box<dyn Decoder>), String> {
        self.state = NegotiationState::Probing;
        let total = tracks.len();

        if total == 0 {
            let err = "No tracks present in media container".to_string();
            self.state = NegotiationState::Failed { reason: err.clone() };
            return Err(err);
        }

        for (index, track) in tracks.iter().enumerate() {
            self.state = NegotiationState::InspectingTrack { index, total };

            // 映像トラック等の明らかな非音声トラック（サンプルレートなし、extra_dataなし、channelsなし）をスキップ
            if track.codec_params.sample_rate.is_none()
                && track.codec_params.channels.is_none()
                && track.codec_params.extra_data.is_none()
            {
                continue;
            }

            self.state = NegotiationState::SanitizingParams { track_id: track.id };
            let sanitized_params = Self::sanitize_codec_parameters(&track.codec_params);

            self.state = NegotiationState::InstantiatingDecoder { track_id: track.id };
            match registry.make(&sanitized_params, options) {
                Ok(decoder) => {
                    self.state = NegotiationState::Ready { track_id: track.id };
                    let mut resolved_track = track.clone();
                    resolved_track.codec_params = sanitized_params;
                    crate::logger::info(
                        "TrackNegotiationFsm",
                        &format!("Audio track successfully negotiated: track_id={}, codec={:?}", track.id, resolved_track.codec_params.codec),
                    );
                    return Ok((resolved_track, decoder));
                }
                Err(e) => {
                    crate::logger::warn(
                        "TrackNegotiationFsm",
                        &format!("Decoder instantiation failed for track {}: {:?}, attempting fallback to next track", track.id, e),
                    );
                }
            }
        }

        let err = "No playable audio track found in media container".to_string();
        self.state = NegotiationState::Failed { reason: err.clone() };
        Err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::codecs::{CodecParameters, CODEC_TYPE_OPUS};

    #[test]
    fn test_sanitize_codec_parameters_opus_head() {
        let mut params = CodecParameters::new();
        params.codec = CODEC_TYPE_OPUS;
        params.channels = None;
        params.sample_rate = None;

        // RFC 7845 準拠のダミー OpusHead バイナリ (Stereo, 48kHz)
        let mut opus_head = Vec::new();
        opus_head.extend_from_slice(b"OpusHead"); // 0..8
        opus_head.push(1); // version
        opus_head.push(2); // channel_count = 2 (Stereo)
        opus_head.extend_from_slice(&[0, 0]); // pre-skip
        opus_head.extend_from_slice(&[0x80, 0xbb, 0, 0]); // 48000 Hz
        opus_head.extend_from_slice(&[0, 0]); // output_gain
        opus_head.push(0); // channel_mapping_family

        params.extra_data = Some(opus_head.into_boxed_slice());

        let sanitized = TrackNegotiationFsm::sanitize_codec_parameters(&params);
        assert_eq!(
            sanitized.channels,
            Some(Channels::FRONT_LEFT | Channels::FRONT_RIGHT)
        );
        assert_eq!(sanitized.sample_rate, Some(48000));
    }

    #[test]
    fn test_track_negotiation_fsm_empty_tracks_fails() {
        let mut fsm = TrackNegotiationFsm::new();
        let registry = CodecRegistry::new();
        let res = fsm.negotiate(&[], &registry, &DecoderOptions::default());
        assert!(res.is_err());
        assert!(matches!(fsm.state(), NegotiationState::Failed { .. }));
    }
}
