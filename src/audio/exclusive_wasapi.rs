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
    actual_sample_rate: u32,
    native_sample_rate: u32,
}

#[cfg(windows)]
fn make_wave_format_ext(sample_rate: u32, channels: u16, is_float: bool) -> windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE {
    use windows::core::GUID;
    use windows::Win32::Media::Audio::*;

    let guid = if is_float {
        GUID::from_values(0x00000003, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71])
    } else {
        GUID::from_values(0x00000001, 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71])
    };
    let bits = if is_float { 32 } else { 16 };
    let bytes_per_sample = bits / 8;
    let channel_mask = if channels == 1 { 0x4 } else { 0x3 };

    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: 0xFFFEu16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * (channels as u32) * (bytes_per_sample as u32),
            nBlockAlign: channels * bytes_per_sample,
            wBitsPerSample: bits,
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: bits,
        },
        dwChannelMask: channel_mask,
        SubFormat: guid,
    }
}

impl ExclusiveBackend {
    pub fn is_supported() -> bool {
        cfg!(windows)
    }

    pub fn actual_sample_rate(&self) -> u32 {
        self.actual_sample_rate
    }

    pub fn native_sample_rate(&self) -> u32 {
        self.native_sample_rate
    }

    pub fn is_bit_perfect(&self) -> bool {
        self.actual_sample_rate == self.native_sample_rate
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

        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(u32, bool), String>>();

        logger::info(
            "ExclusiveBackend",
            &format!(
                "Attempting WASAPI Exclusive Mode initialization: rate={}Hz, channels={}",
                sample_rate, channels
            ),
        );

        let target_device_name = _device_name.to_string();

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

                let device: Result<IMMDevice, _> = if target_device_name.is_empty() || target_device_name == "Default" {
                    enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                } else {
                    let mut found = None;
                    if let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                        if let Ok(count) = collection.GetCount() {
                            for i in 0..count {
                                if let Ok(dev) = collection.Item(i) {
                                    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
                                    use windows::Win32::System::Com::STGM_READ;
                                    let pkey_friendly_name = PROPERTYKEY {
                                        fmtid: GUID::from_values(0xa45c254e, 0xdf1c, 0x4efd, [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0]),
                                        pid: 14,
                                    };
                                    if let Ok(store) = dev.OpenPropertyStore(STGM_READ) {
                                        if let Ok(prop) = store.GetValue(&pkey_friendly_name) {
                                            let name_str = prop.to_string();
                                            if name_str.contains(&target_device_name) || target_device_name.contains(&name_str) {
                                                found = Some(dev);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(dev) = found {
                        logger::info("ExclusiveBackend", &format!("Selected requested audio device: '{}'", target_device_name));
                        Ok(dev)
                    } else {
                        logger::warn("ExclusiveBackend", &format!("Specified device '{}' not found, falling back to default.", target_device_name));
                        enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                    }
                };

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

                // 1. ネイティブレート（Float 32-bit / PCM 16-bit）の排他サポート判定
                let mut chosen_rate = sample_rate;
                let mut is_float = true;
                let mut chosen_format_ext = make_wave_format_ext(sample_rate, channels, true);

                let mut is_supported = client.IsFormatSupported(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    &chosen_format_ext as *const _ as *const WAVEFORMATEX,
                    None,
                ).is_ok();

                if !is_supported {
                    let pcm_format = make_wave_format_ext(sample_rate, channels, false);
                    if client.IsFormatSupported(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        &pcm_format as *const _ as *const WAVEFORMATEX,
                        None,
                    ).is_ok() {
                        is_supported = true;
                        is_float = false;
                        chosen_format_ext = pcm_format;
                    }
                }

                // 2. ネイティブレート非対応時の自動リサンプリングターゲット探索
                if !is_supported {
                    logger::warn(
                        "ExclusiveBackend",
                        &format!(
                            "Device does not support native rate {}Hz directly in Exclusive mode. Probing fallback rates...",
                            sample_rate
                        ),
                    );

                    // 音源に近い順・一般的なハイレゾ/CD音質順に候補レートを走査
                    let candidates = [192000, 96000, 48000, 44100, 88200, 176400];
                    for &cand in &candidates {
                        if cand == sample_rate {
                            continue;
                        }
                        let fmt_fl = make_wave_format_ext(cand, channels, true);
                        if client.IsFormatSupported(
                            AUDCLNT_SHAREMODE_EXCLUSIVE,
                            &fmt_fl as *const _ as *const WAVEFORMATEX,
                            None,
                        ).is_ok() {
                            chosen_rate = cand;
                            is_float = true;
                            chosen_format_ext = fmt_fl;
                            is_supported = true;
                            logger::info(
                                "ExclusiveBackend",
                                &format!("Found supported Exclusive fallback rate: {}Hz (Float 32-bit)", cand),
                            );
                            break;
                        }
                        let fmt_pcm = make_wave_format_ext(cand, channels, false);
                        if client.IsFormatSupported(
                            AUDCLNT_SHAREMODE_EXCLUSIVE,
                            &fmt_pcm as *const _ as *const WAVEFORMATEX,
                            None,
                        ).is_ok() {
                            chosen_rate = cand;
                            is_float = false;
                            chosen_format_ext = fmt_pcm;
                            is_supported = true;
                            logger::info(
                                "ExclusiveBackend",
                                &format!("Found supported Exclusive fallback rate: {}Hz (PCM 16-bit)", cand),
                            );
                            break;
                        }
                    }
                }

                if !is_supported {
                    let err_msg = format!("Device does not support any exclusive format for {} channels", channels);
                    logger::error("ExclusiveBackend", &err_msg);
                    let _ = init_tx.send(Err(err_msg));
                    CoUninitialize();
                    return;
                }

                let p_format = &chosen_format_ext as *const _ as *const WAVEFORMATEX;
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
                            period_to_use = ((aligned_frames as f64 / chosen_rate as f64 * 10_000_000.0) + 0.5) as i64;
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

                let is_bit_perfect = chosen_rate == sample_rate;
                logger::info(
                    "ExclusiveBackend",
                    &format!(
                        "Exclusive mode stream started successfully: native={}Hz, actual={}Hz, bit_perfect={}",
                        sample_rate, chosen_rate, is_bit_perfect
                    ),
                );
                let _ = init_tx.send(Ok((chosen_rate, is_bit_perfect)));

                let mut resampler = crate::audio::resampler::AudioResampler::new(sample_rate, chosen_rate, channels);
                let mut resampled_fifo = std::collections::VecDeque::<f32>::with_capacity(total_buffer_samples * 2);

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

                    if state.seek_trigger.swap(false, Ordering::Acquire) {
                        while consumer.pop().is_ok() {}
                        resampled_fifo.clear();
                        resampler.reset();
                    }

                    if let Ok(p_data) = render_client.GetBuffer(buffer_frame_count) {
                        if is_float {
                            let slice = std::slice::from_raw_parts_mut(p_data as *mut f32, total_buffer_samples);
                            if is_active {
                                let mut frames_read = 0;
                                if resampler.is_resampling_needed() {
                                    while resampled_fifo.len() < total_buffer_samples {
                                        let mut chunk = Vec::with_capacity(256 * ch_count);
                                        for _ in 0..(256 * ch_count) {
                                            match consumer.pop() {
                                                Ok(s) => chunk.push(s),
                                                Err(_) => break,
                                            }
                                        }
                                        if chunk.is_empty() {
                                            break;
                                        }
                                        let out_chunk = resampler.process(&chunk);
                                        for s in out_chunk {
                                            resampled_fifo.push_back(s);
                                        }
                                    }

                                    for frame in slice.chunks_mut(ch_count) {
                                        let has_sample = resampled_fifo.len() >= ch_count;
                                        for ch_sample in frame.iter_mut() {
                                            let s = resampled_fifo.pop_front().unwrap_or(0.0);
                                            *ch_sample = s * vol;
                                        }
                                        if has_sample {
                                            frames_read += 1;
                                        }
                                    }
                                } else {
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
                                }
                                if frames_read > 0 {
                                    let native_frames = if resampler.is_resampling_needed() {
                                        ((frames_read as f64 * sample_rate as f64 / chosen_rate as f64) + 0.5) as u64
                                    } else {
                                        frames_read as u64
                                    };
                                    state.current_sample_position.fetch_add(native_frames, Ordering::Relaxed);
                                    state.push_visualizer_samples(slice);
                                }
                            } else {
                                slice.fill(0.0);
                            }
                        } else {
                            let slice = std::slice::from_raw_parts_mut(p_data as *mut i16, total_buffer_samples);
                            if is_active {
                                let mut frames_read = 0;
                                if resampler.is_resampling_needed() {
                                    while resampled_fifo.len() < total_buffer_samples {
                                        let mut chunk = Vec::with_capacity(256 * ch_count);
                                        for _ in 0..(256 * ch_count) {
                                            match consumer.pop() {
                                                Ok(s) => chunk.push(s),
                                                Err(_) => break,
                                            }
                                        }
                                        if chunk.is_empty() {
                                            break;
                                        }
                                        let out_chunk = resampler.process(&chunk);
                                        for s in out_chunk {
                                            resampled_fifo.push_back(s);
                                        }
                                    }

                                    for frame in slice.chunks_mut(ch_count) {
                                        let has_sample = resampled_fifo.len() >= ch_count;
                                        for ch_sample in frame.iter_mut() {
                                            let s = resampled_fifo.pop_front().unwrap_or(0.0);
                                            let scaled = (s * vol * 32767.0).clamp(-32768.0, 32767.0);
                                            *ch_sample = scaled as i16;
                                        }
                                        if has_sample {
                                            frames_read += 1;
                                        }
                                    }
                                } else {
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
                                }
                                if frames_read > 0 {
                                    let native_frames = if resampler.is_resampling_needed() {
                                        ((frames_read as f64 * sample_rate as f64 / chosen_rate as f64) + 0.5) as u64
                                    } else {
                                        frames_read as u64
                                    };
                                    state.current_sample_position.fetch_add(native_frames, Ordering::Relaxed);
                                    let mut f32_chunk = [0.0f32; 1024];
                                    for chunk in slice.chunks(f32_chunk.len()) {
                                        for (out_s, &in_s) in f32_chunk.iter_mut().zip(chunk.iter()) {
                                            *out_s = in_s as f32 / 32768.0;
                                        }
                                        state.push_visualizer_samples(&f32_chunk[..chunk.len()]);
                                    }
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
            Ok(Ok((actual_sample_rate, _))) => {
                logger::info(
                    "ExclusiveBackend",
                    &format!(
                        "ExclusiveBackend initialized: native={}Hz, actual={}Hz",
                        sample_rate, actual_sample_rate
                    ),
                );
                Ok(Self {
                    is_running,
                    stop_signal,
                    wake_event_handle,
                    thread_handle: Some(thread_handle),
                    actual_sample_rate,
                    native_sample_rate: sample_rate,
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

    #[test]
    fn test_exclusive_backend_rate_accessors() {
        let backend = ExclusiveBackend {
            is_running: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            wake_event_handle: Arc::new(AtomicIsize::new(0)),
            thread_handle: None,
            actual_sample_rate: 48000,
            native_sample_rate: 48000,
        };
        assert_eq!(backend.actual_sample_rate(), 48000);
        assert_eq!(backend.native_sample_rate(), 48000);
        assert!(backend.is_bit_perfect());

        let resampled_backend = ExclusiveBackend {
            is_running: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            wake_event_handle: Arc::new(AtomicIsize::new(0)),
            thread_handle: None,
            actual_sample_rate: 48000,
            native_sample_rate: 44100,
        };
        assert_eq!(resampled_backend.actual_sample_rate(), 48000);
        assert_eq!(resampled_backend.native_sample_rate(), 44100);
        assert!(!resampled_backend.is_bit_perfect());
    }

    #[cfg(windows)]
    #[test]
    fn test_make_wave_format_ext_properties() {
        let fmt_float = make_wave_format_ext(48000, 2, true);
        let rate_float = fmt_float.Format.nSamplesPerSec;
        let channels_float = fmt_float.Format.nChannels;
        let bits_float = fmt_float.Format.wBitsPerSample;
        assert_eq!(rate_float, 48000);
        assert_eq!(channels_float, 2);
        assert_eq!(bits_float, 32);

        let fmt_pcm = make_wave_format_ext(96000, 2, false);
        let rate_pcm = fmt_pcm.Format.nSamplesPerSec;
        let channels_pcm = fmt_pcm.Format.nChannels;
        let bits_pcm = fmt_pcm.Format.wBitsPerSample;
        assert_eq!(rate_pcm, 96000);
        assert_eq!(channels_pcm, 2);
        assert_eq!(bits_pcm, 16);
    }
}
