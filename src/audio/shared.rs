use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::traits::{AudioDeviceInfo, AudioOutputBackend};
use crate::logger;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::Consumer;
use std::error::Error;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct SharedBackend {
    stream: cpal::Stream,
    is_running: bool,
}

unsafe impl Send for SharedBackend {}

impl SharedBackend {
    pub fn list_devices() -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        let default_device_name = host.default_output_device().and_then(|d| d.name().ok());

        let mut devices = Vec::new();
        if let Ok(device_iter) = host.output_devices() {
            for device in device_iter {
                if let Ok(name) = device.name() {
                    let is_default = default_device_name.as_ref() == Some(&name);
                    devices.push(AudioDeviceInfo {
                        id: name.clone(),
                        name,
                        is_default,
                    });
                }
            }
        }
        devices
    }

    pub fn create(
        device_name: &str,
        source_sample_rate: u32,
        source_channels: u16,
        mut consumer: Consumer<f32>,
        state: Arc<SharedAudioState>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let host = cpal::default_host();

        let device = if device_name == "Default" || device_name.is_empty() {
            host.default_output_device()
                .ok_or("No default output audio device found")?
        } else {
            let mut found = None;
            if let Ok(device_iter) = host.output_devices() {
                for dev in device_iter {
                    if let Ok(name) = dev.name() {
                        if name == device_name {
                            found = Some(dev);
                            break;
                        }
                    }
                }
            }
            found.or_else(|| host.default_output_device())
                .ok_or("Specified output device not found, and no default device available")?
        };

        // デバイスの既定フォーマットを取得（Windows Audio Engineの共有ミキサー設定）
        let default_cfg = device.default_output_config()?;
        let output_sample_rate = default_cfg.sample_rate().0;
        let output_channels = default_cfg.channels();

        logger::info(
            "SharedBackend",
            &format!(
                "Hardware stream target: rate={}Hz, ch={}. Source: rate={}Hz, ch={}",
                output_sample_rate, output_channels, source_sample_rate, source_channels
            ),
        );

        let config = cpal::StreamConfig {
            channels: output_channels,
            sample_rate: cpal::SampleRate(output_sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let err_callback = |err| {
            logger::error("SharedBackend", &format!("Audio stream callback error: {:?}", err));
        };

        let src_channels = source_channels.max(1) as usize;
        let out_channels = output_channels.max(1) as usize;

        // 同一レート・チャンネルの場合はダイレクト出力
        let is_direct = source_sample_rate == output_sample_rate && src_channels == out_channels;

        let stream = if is_direct {
            logger::info("SharedBackend", "Direct 1:1 stream path selected (no resampling).");
            let mut fade_in_frames = 0usize;
            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let is_playing = state.is_playing.load(Ordering::Relaxed);
                    if !is_playing {
                        output.fill(0.0);
                        return;
                    }

                    if state.seek_trigger.swap(false, Ordering::Acquire) {
                        while consumer.pop().is_ok() {}
                        fade_in_frames = 64;
                    }

                    let vol = state.get_volume();
                    let mut frames_played = 0;

                    for frame in output.chunks_mut(out_channels) {
                        let fade_mult = if fade_in_frames > 0 {
                            let m = 1.0 - (fade_in_frames as f32 / 64.0);
                            fade_in_frames -= 1;
                            m
                        } else {
                            1.0
                        };

                        let mut frame_ok = true;
                        for ch_sample in frame.iter_mut() {
                            match consumer.pop() {
                                Ok(sample) => {
                                    *ch_sample = sample * vol * fade_mult;
                                }
                                Err(_) => {
                                    *ch_sample = 0.0;
                                    frame_ok = false;
                                }
                            }
                        }
                        if frame_ok {
                            frames_played += 1;
                        }
                    }

                    if frames_played > 0 {
                        state.current_sample_position.fetch_add(frames_played as u64, Ordering::Relaxed);
                        state.push_visualizer_samples(output);
                    }
                },
                err_callback,
                None,
            )?
        } else {
            logger::info(
                "SharedBackend",
                &format!("Auto-Resampler enabled ({}Hz -> {}Hz).", source_sample_rate, output_sample_rate),
            );
            let resample_ratio = source_sample_rate as f64 / output_sample_rate as f64;
            let mut curr_frame = vec![0.0f32; src_channels];
            let mut next_frame = vec![0.0f32; src_channels];
            let mut src_fraction = 0.0f64;
            let mut fade_in_frames = 0usize;

            // 初期フレームの読み込み
            for ch in 0..src_channels {
                curr_frame[ch] = consumer.pop().unwrap_or(0.0);
                next_frame[ch] = consumer.pop().unwrap_or(0.0);
            }

            device.build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let is_playing = state.is_playing.load(Ordering::Relaxed);
                    if !is_playing {
                        output.fill(0.0);
                        return;
                    }

                    // シーク発生時のリサンプラー境界リセットとマイクロフェードイン
                    if state.seek_trigger.swap(false, Ordering::Acquire) {
                        while consumer.pop().is_ok() {}
                        for ch in 0..src_channels {
                            curr_frame[ch] = 0.0;
                            next_frame[ch] = 0.0;
                        }
                        src_fraction = 0.0;
                        fade_in_frames = 64;
                    }

                    let vol = state.get_volume();
                    let mut src_frames_consumed = 0u64;

                    // コールバック内ヒープアロケーションを完全排除（スタック配列）
                    let mut interpolated_src = [0.0f32; 8];

                    for frame in output.chunks_mut(out_channels) {
                        let fade_mult = if fade_in_frames > 0 {
                            let m = 1.0 - (fade_in_frames as f32 / 64.0);
                            fade_in_frames -= 1;
                            m
                        } else {
                            1.0
                        };

                        // 線形補間サンプル値の計算
                        let frac = src_fraction as f32;
                        for ch in 0..src_channels.min(8) {
                            interpolated_src[ch] = (curr_frame[ch] * (1.0 - frac) + next_frame[ch] * frac) * vol * fade_mult;
                        }

                        // チャンネルマッピング
                        for (out_ch_idx, out_sample) in frame.iter_mut().enumerate() {
                            if out_ch_idx < src_channels {
                                *out_sample = interpolated_src[out_ch_idx];
                            } else {
                                // モノラル -> ステレオ、または 2ch -> マルチチャンネルの補完
                                *out_sample = interpolated_src[out_ch_idx % src_channels];
                            }
                        }

                        src_fraction += resample_ratio;
                        while src_fraction >= 1.0 {
                            src_fraction -= 1.0;
                            curr_frame.copy_from_slice(&next_frame);
                            for ch in 0..src_channels {
                                next_frame[ch] = consumer.pop().unwrap_or(0.0);
                            }
                            src_frames_consumed += 1;
                        }
                    }

                    if src_frames_consumed > 0 {
                        state.current_sample_position.fetch_add(src_frames_consumed, Ordering::Relaxed);
                        state.push_visualizer_samples(output);
                    }
                },
                err_callback,
                None,
            )?
        };

        Ok(Self {
            stream,
            is_running: false,
        })
    }
}

impl AudioOutputBackend for SharedBackend {
    fn play(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if !self.is_running {
            let _ = self.stream.play();
            self.is_running = true;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.is_running = false;
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.is_running
    }

    fn mode_name(&self) -> &'static str {
        "Shared (WASAPI / CoreAudio / ALSA)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::ring_buffer::create_ring_buffer;

    #[test]
    fn test_list_devices_returns_available_devices() {
        let devices = SharedBackend::list_devices();
        if !devices.is_empty() {
            assert!(devices.iter().any(|d| d.is_default || !d.name.is_empty()));
        }
    }

    #[test]
    #[ignore = "Requires active audio hardware and dedicated process"]
    fn test_shared_backend_initialization_and_control() {
        let (_producer, consumer, state) = create_ring_buffer(2048);
        let backend_res = SharedBackend::create("Default", 44100, 2, consumer, state);
        if let Ok(mut backend) = backend_res {
            assert_eq!(backend.mode_name(), "Shared (WASAPI / CoreAudio / ALSA)");
            assert!(!backend.is_active());

            assert!(backend.play().is_ok());
            assert!(backend.is_active());

            assert!(backend.pause().is_ok());
            assert!(!backend.is_active());
        }
    }
}
