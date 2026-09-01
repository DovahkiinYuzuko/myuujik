use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::traits::AudioOutputBackend;
use crate::logger;
use rtrb::Consumer;
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;

pub struct ExclusiveBackend {
    is_running: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    wake_event_handle: Arc<AtomicIsize>,
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
        let wake_event_handle = Arc::new(AtomicIsize::new(0));

        let is_running_clone = Arc::clone(&is_running);
        let stop_signal_clone = Arc::clone(&stop_signal);
        let wake_event_handle_clone = Arc::clone(&wake_event_handle);

        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        let guid_float = GUID::from_values(0x00000003, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]);
        let guid_pcm = GUID::from_values(0x00000001, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71]);

        logger::info(
            "ExclusiveBackend",
            &format!(
                "Attempting WASAPI Exclusive Mode initialization: rate={}Hz, channels={}",
                sample_rate, channels
            ),
        );

        let thread_handle = std::thread::spawn(move || {
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    let err_msg = "CoInitializeEx failed";
                    logger::error("ExclusiveBackend", err_msg);
                    let _ = init_tx.send(Err(err_msg.to_string()));
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
                        let err_msg = format!("MMDeviceEnumerator failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
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
                        let err_msg = format!("GetDefaultAudioEndpoint failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
                        CoUninitialize();
                        return;
                    }
                };

                let client: Result<IAudioClient, _> = device.Activate(CLSCTX_ALL, None);
                let mut client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        let err_msg = format!("IAudioClient Activate failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
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

                logger::info(
                    "ExclusiveBackend",
                    &format!(
                        "Hardware period queried: default={} ({}ms), min={}",
                        default_period,
                        default_period as f64 / 10_000.0,
                        min_period
                    ),
                );

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

                // 2. 16-bit PCM フォーマットのフォールバック定義
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
                    logger::warn("ExclusiveBackend", "IEEE Float 32-bit not supported directly, trying 16-bit PCM...");
                    is_float = false;
                    p_format = &format_ext_pcm as *const _ as *const WAVEFORMATEX;

                    let pcm_check = client.IsFormatSupported(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        p_format,
                        None,
                    );
                    if pcm_check.is_err() {
                        let err_msg = format!("Device does not support requested format in Exclusive mode: {:?}", pcm_check);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
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
                            logger::info(
                                "ExclusiveBackend",
                                &format!("Re-aligning buffer size to {} frames (period={})", aligned_frames, period_to_use),
                            );
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
                    let err_msg = format!("IAudioClient Initialize failed: {:?}", e);
                    logger::error("ExclusiveBackend", &err_msg);
                    let _ = init_tx.send(Err(err_msg));
                    CoUninitialize();
                    return;
                }

                let buffer_frame_count = match client.GetBufferSize() {
                    Ok(b) => b,
                    Err(e) => {
                        let err_msg = format!("GetBufferSize failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
                        CoUninitialize();
                        return;
                    }
                };

                let event_handle = match CreateEventW(None, false, false, None) {
                    Ok(h) => {
                        wake_event_handle_clone.store(h.0 as isize, Ordering::Relaxed);
                        h
                    }
                    Err(e) => {
                        let err_msg = format!("CreateEventW failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
                        CoUninitialize();
                        return;
                    }
                };

                if let Err(e) = client.SetEventHandle(event_handle) {
                    let err_msg = format!("SetEventHandle failed: {:?}", e);
                    logger::error("ExclusiveBackend", &err_msg);
                    let _ = init_tx.send(Err(err_msg));
                    wake_event_handle_clone.store(0, Ordering::Relaxed);
                    let _ = CloseHandle(event_handle);
                    CoUninitialize();
                    return;
                }

                let render_client: Result<IAudioRenderClient, _> = client.GetService();
                let render_client = match render_client {
                    Ok(r) => r,
                    Err(e) => {
                        let err_msg = format!("GetService<IAudioRenderClient> failed: {:?}", e);
                        logger::error("ExclusiveBackend", &err_msg);
                        let _ = init_tx.send(Err(err_msg));
                        wake_event_handle_clone.store(0, Ordering::Relaxed);
                        let _ = CloseHandle(event_handle);
                        CoUninitialize();
                        return;
                    }
                };

                let mut consumer = consumer;
                let ch_count = channels as usize;
                let total_buffer_samples = (buffer_frame_count as usize) * ch_count;

                // 再生前初期バッファ充填（サイレント・プリバッファリング）
                if let Ok(p_data) = render_client.GetBuffer(buffer_frame_count) {
                    if is_float {
                        let slice = std::slice::from_raw_parts_mut(p_data as *mut f32, total_buffer_samples);
                        slice.fill(0.0);
                    } else {
                        let slice = std::slice::from_raw_parts_mut(p_data as *mut i16, total_buffer_samples);
                        slice.fill(0);
                    }
                    let _ = render_client.ReleaseBuffer(buffer_frame_count, 0);
                }

                if let Err(e) = client.Start() {
                    let err_msg = format!("IAudioClient Start failed: {:?}", e);
                    logger::error("ExclusiveBackend", &err_msg);
                    let _ = init_tx.send(Err(err_msg));
                    wake_event_handle_clone.store(0, Ordering::Relaxed);
                    let _ = CloseHandle(event_handle);
                    CoUninitialize();
                    return;
                }

                logger::info("ExclusiveBackend", "Exclusive mode stream started successfully.");
                let _ = init_tx.send(Ok(()));

                // イベントドリブン WASAPI 再生ループ（タイムアウト50ms＋即時停止検知）
                while !stop_signal_clone.load(Ordering::Relaxed) {
                    let wait_res = WaitForSingleObject(event_handle, 50);
                    if stop_signal_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    if wait_res != WAIT_OBJECT_0 {
                        continue;
                    }

                    let is_active = is_running_clone.load(Ordering::Relaxed);
                    let vol = state.get_volume();

                    if let Ok(p_data) = render_client.GetBuffer(buffer_frame_count) {
                        if is_float {
                            let slice = std::slice::from_raw_parts_mut(p_data as *mut f32, total_buffer_samples);
                            if is_active {
                                let mut frames_read = 0;
                                for frame in slice.chunks_mut(ch_count) {
                                    let mut frame_ok = true;
                                    for ch_sample in frame.iter_mut() {
                                        match consumer.pop() {
                                            Ok(s) => {
                                                *ch_sample = s * vol;
                                            }
                                            Err(_) => {
                                                *ch_sample = 0.0;
                                                frame_ok = false;
                                            }
                                        }
                                    }
                                    if frame_ok {
                                        frames_read += 1;
                                    }
                                }
                                if frames_read > 0 {
                                    state.current_sample_position.fetch_add(frames_read as u64, Ordering::Relaxed);
                                }
                            } else {
                                slice.fill(0.0);
                            }
                        } else {
                            let slice = std::slice::from_raw_parts_mut(p_data as *mut i16, total_buffer_samples);
                            if is_active {
                                let mut frames_read = 0;
                                for frame in slice.chunks_mut(ch_count) {
                                    let mut frame_ok = true;
                                    for ch_sample in frame.iter_mut() {
                                        match consumer.pop() {
                                            Ok(s) => {
                                                let scaled = (s * vol * 32767.0).clamp(-32768.0, 32767.0);
                                                *ch_sample = scaled as i16;
                                            }
                                            Err(_) => {
                                                *ch_sample = 0;
                                                frame_ok = false;
                                            }
                                        }
                                    }
                                    if frame_ok {
                                        frames_read += 1;
                                    }
                                }
                                if frames_read > 0 {
                                    state.current_sample_position.fetch_add(frames_read as u64, Ordering::Relaxed);
                                }
                            } else {
                                slice.fill(0);
                            }
                        }

                        let _ = render_client.ReleaseBuffer(buffer_frame_count, 0);
                    }
                }

                let _ = client.Stop();
                wake_event_handle_clone.store(0, Ordering::Relaxed);
                let _ = CloseHandle(event_handle);
                CoUninitialize();
                logger::info("ExclusiveBackend", "Exclusive mode stream stopped and cleaned up.");
            }
        });

        match init_rx.recv() {
            Ok(Ok(())) => {
                Ok(Self {
                    is_running,
                    stop_signal,
                    wake_event_handle,
                    thread_handle: Some(thread_handle),
                })
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err("Exclusive audio initialization thread panicked or disconnected".into()),
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
        Err("WASAPI Exclusive Mode is only supported on Windows".into())
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
        "Exclusive (WASAPI Event Bit-Perfect)"
    }
}

impl Drop for ExclusiveBackend {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        {
            let raw_h = self.wake_event_handle.load(Ordering::Relaxed);
            if raw_h != 0 {
                use windows::Win32::Foundation::HANDLE;
                use windows::Win32::System::Threading::SetEvent;
                unsafe {
                    let _ = SetEvent(HANDLE(raw_h as _));
                }
            }
        }
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
