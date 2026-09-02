use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::traits::AudioOutputBackend;
use rtrb::Consumer;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct MockAudioBackend {
    is_running: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MockAudioBackend {
    pub fn create(
        mut consumer: Consumer<f32>,
        state: Arc<SharedAudioState>,
    ) -> Self {
        let is_running = Arc::new(AtomicBool::new(false));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let is_running_clone = Arc::clone(&is_running);
        let stop_signal_clone = Arc::clone(&stop_signal);

        let thread = thread::spawn(move || {
            let mut buf = [0.0f32; 1024];
            while !stop_signal_clone.load(Ordering::Relaxed) {
                if is_running_clone.load(Ordering::Relaxed) {
                    let mut read = 0;
                    while read < buf.len() {
                        if let Ok(sample) = consumer.pop() {
                            buf[read] = sample;
                            read += 1;
                        } else {
                            break;
                        }
                    }

                    if read > 0 {
                        let vol = state.get_volume();
                        for s in &mut buf[..read] {
                            *s *= vol;
                        }
                        // ステレオ想定（2chで割る）
                        state.current_sample_position.fetch_add(read as u64 / 2, Ordering::Relaxed);
                        state.push_visualizer_samples(&buf[..read]);
                    }
                }
                thread::sleep(Duration::from_millis(5));
            }
        });

        Self {
            is_running,
            stop_signal,
            thread: Some(thread),
        }
    }
}

impl AudioOutputBackend for MockAudioBackend {
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
        "Mock (In-Memory Audio Sink)"
    }
}

impl Drop for MockAudioBackend {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
