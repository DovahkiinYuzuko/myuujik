use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::traits::{AudioDeviceInfo, AudioOutputBackend};
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
        sample_rate: u32,
        channels: u16,
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

        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let err_callback = |err| {
            eprintln!("Audio stream error: {:?}", err);
        };

        let stream = device.build_output_stream(
            &config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let is_playing = state.is_playing.load(Ordering::Relaxed);
                if !is_playing {
                    output.fill(0.0);
                    return;
                }

                let vol = state.get_volume();
                let ch_count = channels.max(1) as usize;
                let mut frames_played = 0;

                for frame in output.chunks_mut(ch_count) {
                    let mut frame_ok = true;
                    for ch_sample in frame.iter_mut() {
                        match consumer.pop() {
                            Ok(sample) => {
                                *ch_sample = sample * vol;
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
                }
            },
            err_callback,
            None,
        )?;

        Ok(Self {
            stream,
            is_running: false,
        })
    }
}

impl AudioOutputBackend for SharedBackend {
    fn play(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.stream.play()?;
        self.is_running = true;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.stream.pause()?;
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
