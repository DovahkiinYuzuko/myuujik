use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::traits::AudioOutputBackend;
use rtrb::Consumer;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct ExclusiveBackend {
    is_running: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl ExclusiveBackend {
    pub fn is_supported() -> bool {
        cfg!(windows)
    }

    #[cfg(windows)]
    pub fn create(
        _device_name: &str,
        sample_rate: u32,
        channels: u16,
        _bits_per_sample: u16,
        consumer: Consumer<f32>,
        state: Arc<SharedAudioState>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        use windows::core::GUID;
        use windows::Win32::Foundation::*;
        use windows::Win32::Media::Audio::*;
        use windows::Win32::System::Com::*;
        use windows::Win32::System::Threading::*;

        let is_running = Arc::new(AtomicBool::new(false));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let is_running_clone = Arc::clone(&is_running);
        let stop_signal_clone = Arc::clone(&stop_signal);

        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        let guid_float = GUID::from_values(0x00000003, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]);
        let guid_pcm = GUID::from_values(0x00000001, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]);

        let thread_handle = std::thread::spawn(move || {
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    let _ = init_tx.send(Err("CoInitializeEx failed".to_string()));
                    return;
                }

                let enumerator: Result<IMMDeviceEnumerator, _> = CoCreateInstance(
                    &MMDeviceEnumerator,
                    None,
                    CLSCTX_ALL,
                );

                let enumerator = match enumerator {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("MMDeviceEnumerator failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                let device: Result<IMMDevice, _> = enumerator.GetDefaultAudioEndpoint(
                    eRender,
                    eConsole,
                );

                let device = match device {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("GetDefaultAudioEndpoint failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                let client: Result<IAudioClient, _> = device.Activate(CLSCTX_ALL, None);
                let mut client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("IAudioClient Activate failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                // ハードウェアデバイス周期の取得
                let mut default_period: i64 = 0;
                let mut min_period: i64 = 0;
                if client.GetDevicePeriod(Some(&mut default_period), Some(&mut min_period)).is_err() || default_period == 0 {
                    default_period = 100_000; // 10ms fallback
                }

                // 1. IEEE Float 32-bit フォーマットの構築を試行
                let channel_mask = if channels == 1 { 0x4 } else { 0x3 };
                let format_ext_float = WAVEFORMATEXTENSIBLE {
                    Format: WAVEFORMATEX {
                        wFormatTag: 0xFFFEu16, // WAVE_FORMAT_EXTENSIBLE
                        nChannels: channels,
                        nSamplesPerSec: sample_rate,
                        nAvgBytesPerSec: sample_rate * (channels as u32) * 4,
                        nBlockAlign: channels * 4,
                        wBitsPerSample: 32,
                        cbSize: 22,
                    },
                    Samples: WAVEFORMATEXTENSIBLE_0 {
                        wValidBitsPerSample: 32,
                    },
                    dwChannelMask: channel_mask,
                    SubFormat: guid_float,
                };

                let format_ext_pcm = WAVEFORMATEXTENSIBLE {
                    Format: WAVEFORMATEX {
                        wFormatTag: 0xFFFEu16,
                        nChannels: channels,
                        nSamplesPerSec: sample_rate,
                        nAvgBytesPerSec: sample_rate * (channels as u32) * 2,
                        nBlockAlign: channels * 2,
                        wBitsPerSample: 16,
                        cbSize: 22,
                    },
                    Samples: WAVEFORMATEXTENSIBLE_0 {
                        wValidBitsPerSample: 16,
                    },
                    dwChannelMask: channel_mask,
                    SubFormat: guid_pcm,
                };

                let mut is_float = true;
                let mut p_format = &format_ext_float as *const _ as *const WAVEFORMATEX;

                let support_check = client.IsFormatSupported(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    p_format,
                    None,
                );

                // Float非対応の場合、16-bit PCM フォーマットを試行
                if support_check.is_err() {
                    is_float = false;
                    p_format = &format_ext_pcm as *const _ as *const WAVEFORMATEX;

                    if client.IsFormatSupported(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        p_format,
                        None,
                    ).is_err() {
                        let _ = init_tx.send(Err("Device does not support requested format in Exclusive mode".to_string()));
                        CoUninitialize();
                        return;
                    }
                }

                let mut period_to_use = default_period;
                let mut init_res = client.Initialize(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    period_to_use,
                    period_to_use,
                    p_format,
                    None,
                );

                // アライメントエラー発生時の再アライメント処理
                if let Err(ref e) = init_res {
                    if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                        if let Ok(aligned_frames) = client.GetBufferSize() {
                            period_to_use = ((aligned_frames as f64 / sample_rate as f64 * 10_000_000.0) + 0.5) as i64;
                            // クライアントを再アクティベートして再初期化
                            if let Ok(c2) = device.Activate::<IAudioClient>(CLSCTX_ALL, None) {
                                client = c2;
                                init_res = client.Initialize(
                                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                    period_to_use,
                                    period_to_use,
                                    p_format,
                                    None,
                                );
                            }
                        }
                    }
                }

                if let Err(e) = init_res {
                    let _ = init_tx.send(Err(format!("Initialize exclusive stream failed: {:?}", e)));
                    CoUninitialize();
                    return;
                }

                let buffer_frames = match client.GetBufferSize() {
                    Ok(bf) => bf,
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("GetBufferSize failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                let audio_event = match CreateEventW(None, false, false, None) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("CreateEventW failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                if let Err(e) = client.SetEventHandle(audio_event) {
                    let _ = CloseHandle(audio_event);
                    let _ = init_tx.send(Err(format!("SetEventHandle failed: {:?}", e)));
                    CoUninitialize();
                    return;
                }

                let render_client: Result<IAudioRenderClient, _> = client.GetService();
                let render_client = match render_client {
                    Ok(rc) => rc,
                    Err(e) => {
                        let _ = CloseHandle(audio_event);
                        let _ = init_tx.send(Err(format!("GetService IAudioRenderClient failed: {:?}", e)));
                        CoUninitialize();
                        return;
                    }
                };

                let mut mut_consumer = consumer;
                let ch_count = channels as usize;
                let total_samples_per_buffer = (buffer_frames as usize) * ch_count;

                // プレバッファリング（再生開始前の先頭バッファ初期充填）
                if let Ok(buffer_ptr) = render_client.GetBuffer(buffer_frames) {
                    let vol = state.get_volume();
                    if is_float {
                        let dest_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, total_samples_per_buffer);
                        for sample in dest_slice.iter_mut() {
                            *sample = mut_consumer.pop().unwrap_or(0.0) * vol;
                        }
                    } else {
                        let dest_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut i16, total_samples_per_buffer);
                        for sample in dest_slice.iter_mut() {
                            let f = mut_consumer.pop().unwrap_or(0.0) * vol;
                            *sample = (f * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        }
                    }
                    let _ = render_client.ReleaseBuffer(buffer_frames, 0);
                    state.current_sample_position.fetch_add(buffer_frames as u64, Ordering::Relaxed);
                }

                if let Err(e) = client.Start() {
                    let _ = CloseHandle(audio_event);
                    let _ = init_tx.send(Err(format!("IAudioClient Start failed: {:?}", e)));
                    CoUninitialize();
                    return;
                }

                // 初期化成功通知
                let _ = init_tx.send(Ok(()));

                // リアルタイム排他再生ループ
                while !stop_signal_clone.load(Ordering::Relaxed) {
                    let wait_res = WaitForSingleObject(audio_event, 1000);
                    if wait_res != WAIT_OBJECT_0 {
                        continue;
                    }

                    if stop_signal_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    let is_active = is_running_clone.load(Ordering::Relaxed) && state.is_playing.load(Ordering::Relaxed);

                    if let Ok(buffer_ptr) = render_client.GetBuffer(buffer_frames) {
                        let vol = if is_active { state.get_volume() } else { 0.0 };

                        if is_float {
                            let dest_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, total_samples_per_buffer);
                            if is_active {
                                for sample in dest_slice.iter_mut() {
                                    *sample = mut_consumer.pop().unwrap_or(0.0) * vol;
                                }
                            } else {
                                dest_slice.fill(0.0);
                            }
                        } else {
                            let dest_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut i16, total_samples_per_buffer);
                            if is_active {
                                for sample in dest_slice.iter_mut() {
                                    let f = mut_consumer.pop().unwrap_or(0.0) * vol;
                                    *sample = (f * 32767.0).clamp(-32768.0, 32767.0) as i16;
                                }
                            } else {
                                dest_slice.fill(0);
                            }
                        }

                        let _ = render_client.ReleaseBuffer(buffer_frames, 0);
                        if is_active {
                            state.current_sample_position.fetch_add(buffer_frames as u64, Ordering::Relaxed);
                        }
                    }
                }

                let _ = client.Stop();
                let _ = CloseHandle(audio_event);
                CoUninitialize();
            }
        });

        // ワーカースレッドの初期化完了待機
        match init_rx.recv_timeout(std::time::Duration::from_millis(1500)) {
            Ok(Ok(())) => {
                Ok(Self {
                    is_running,
                    stop_signal,
                    thread_handle: Some(thread_handle),
                })
            }
            Ok(Err(err_msg)) => {
                stop_signal.store(true, Ordering::Relaxed);
                let _ = thread_handle.join();
                Err(err_msg.into())
            }
            Err(_) => {
                stop_signal.store(true, Ordering::Relaxed);
                let _ = thread_handle.join();
                Err("WASAPI Exclusive initialization timed out".into())
            }
        }
    }

    #[cfg(not(windows))]
    pub fn create(
        _device_name: &str,
        _sample_rate: u32,
        _channels: u16,
        _bits_per_sample: u16,
        _consumer: Consumer<f32>,
        _state: Arc<SharedAudioState>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Err("WASAPI Exclusive Mode is only available on Windows".into())
    }
}

impl AudioOutputBackend for ExclusiveBackend {
    fn play(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.is_running.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.is_running.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }

    fn mode_name(&self) -> &'static str {
        "Exclusive (WASAPI Bit-Perfect)"
    }
}

impl Drop for ExclusiveBackend {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        self.is_running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exclusive_backend_support_query() {
        if cfg!(windows) {
            assert!(ExclusiveBackend::is_supported());
        } else {
            assert!(!ExclusiveBackend::is_supported());
        }
    }
}
