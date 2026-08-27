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
        bits_per_sample: u16,
        consumer: Consumer<f32>,
        state: Arc<SharedAudioState>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        use windows::Win32::Media::Audio::*;
        use windows::Win32::System::Com::*;
        use windows::Win32::System::Threading::*;

        let is_running = Arc::new(AtomicBool::new(false));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let is_running_clone = Arc::clone(&is_running);
        let stop_signal_clone = Arc::clone(&stop_signal);

        let thread_handle = std::thread::spawn(move || {
            unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                    return;
                }

                let enumerator: Result<IMMDeviceEnumerator, _> = CoCreateInstance(
                    &MMDeviceEnumerator,
                    None,
                    CLSCTX_ALL,
                );

                let enumerator = match enumerator {
                    Ok(e) => e,
                    Err(_) => {
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
                    Err(_) => {
                        CoUninitialize();
                        return;
                    }
                };

                let client: Result<IAudioClient, _> = device.Activate(CLSCTX_ALL, None);
                let client = match client {
                    Ok(c) => c,
                    Err(_) => {
                        CoUninitialize();
                        return;
                    }
                };

                let bits = if bits_per_sample == 0 { 16 } else { bits_per_sample };
                let block_align = channels * (bits / 8);
                let avg_bytes = sample_rate * block_align as u32;

                let format = WAVEFORMATEX {
                    wFormatTag: WAVE_FORMAT_PCM as u16,
                    nChannels: channels,
                    nSamplesPerSec: sample_rate,
                    nAvgBytesPerSec: avg_bytes,
                    nBlockAlign: block_align,
                    wBitsPerSample: bits,
                    cbSize: 0,
                };

                let buffer_duration_hns: i64 = 100_000; // 10ms

                let init_res = client.Initialize(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    buffer_duration_hns,
                    buffer_duration_hns,
                    &format,
                    None,
                );

                if init_res.is_err() {
                    CoUninitialize();
                    return;
                }

                let audio_event = match CreateEventW(None, false, false, None) {
                    Ok(h) => h,
                    Err(_) => {
                        CoUninitialize();
                        return;
                    }
                };

                if client.SetEventHandle(audio_event).is_err() {
                    let _ = windows::Win32::Foundation::CloseHandle(audio_event);
                    CoUninitialize();
                    return;
                }

                let render_client: Result<IAudioRenderClient, _> = client.GetService();
                let render_client = match render_client {
                    Ok(rc) => rc,
                    Err(_) => {
                        let _ = windows::Win32::Foundation::CloseHandle(audio_event);
                        CoUninitialize();
                        return;
                    }
                };

                let mut mut_consumer = consumer;
                let _ = client.Start();

                while !stop_signal_clone.load(Ordering::Relaxed) {
                    let wait_res = WaitForSingleObject(audio_event, 2000);
                    if wait_res != windows::Win32::Foundation::WAIT_OBJECT_0 {
                        continue;
                    }

                    if stop_signal_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    if !is_running_clone.load(Ordering::Relaxed) || !state.is_playing.load(Ordering::Relaxed) {
                        continue;
                    }

                    let padding = client.GetCurrentPadding().unwrap_or(0);
                    let buffer_size = client.GetBufferSize().unwrap_or(0);
                    let frames_needed = buffer_size.saturating_sub(padding);

                    if frames_needed == 0 {
                        continue;
                    }

                    if let Ok(buffer_ptr) = render_client.GetBuffer(frames_needed) {
                        let vol = state.get_volume();
                        let total_samples = (frames_needed as usize) * (channels as usize);
                        let dest_slice = std::slice::from_raw_parts_mut(buffer_ptr as *mut i16, total_samples);

                        for sample in dest_slice.iter_mut() {
                            let f32_val = mut_consumer.pop().unwrap_or(0.0) * vol;
                            *sample = (f32_val * 32767.0).clamp(-32768.0, 32767.0) as i16;
                        }

                        let _ = render_client.ReleaseBuffer(frames_needed, 0);
                        state.current_sample_position.fetch_add(frames_needed as u64, Ordering::Relaxed);
                    }
                }

                let _ = client.Stop();
                let _ = windows::Win32::Foundation::CloseHandle(audio_event);
                CoUninitialize();
            }
        });

        Ok(Self {
            is_running,
            stop_signal,
            thread_handle: Some(thread_handle),
        })
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
