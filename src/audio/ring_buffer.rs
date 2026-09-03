use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct SharedAudioState {
    pub is_playing: AtomicBool,
    pub is_muted: AtomicBool,
    pub volume_bits: AtomicU32,
    pub current_sample_position: AtomicU64,
    pub total_samples: AtomicU64,
    pub sample_rate: AtomicU32,
    pub channels: AtomicU32,
    pub seek_trigger: AtomicBool,
    pub visualizer_samples: Mutex<Vec<f32>>,
}

impl SharedAudioState {
    pub fn new() -> Self {
        Self {
            is_playing: AtomicBool::new(false),
            is_muted: AtomicBool::new(false),
            volume_bits: AtomicU32::new(0.85f32.to_bits()),
            current_sample_position: AtomicU64::new(0),
            total_samples: AtomicU64::new(0),
            sample_rate: AtomicU32::new(44100),
            channels: AtomicU32::new(2),
            seek_trigger: AtomicBool::new(false),
            visualizer_samples: Mutex::new(Vec::with_capacity(2048)),
        }
    }

    pub fn get_volume(&self) -> f32 {
        if self.is_muted.load(Ordering::Relaxed) {
            0.0
        } else {
            f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
        }
    }

    pub fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.volume_bits.store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        self.is_muted.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted.load(Ordering::Relaxed)
    }

    pub fn current_position_secs(&self) -> f64 {
        let samples = self.current_sample_position.load(Ordering::Relaxed);
        let rate = self.sample_rate.load(Ordering::Relaxed).max(1) as f64;
        samples as f64 / rate
    }

    pub fn total_duration_secs(&self) -> f64 {
        let samples = self.total_samples.load(Ordering::Relaxed);
        let rate = self.sample_rate.load(Ordering::Relaxed).max(1) as f64;
        samples as f64 / rate
    }

    pub fn push_visualizer_samples(&self, samples: &[f32]) {
        if let Ok(mut buf) = self.visualizer_samples.try_lock() {
            const MAX_VIZ_SAMPLES: usize = 2048;
            buf.extend_from_slice(samples);
            if buf.len() > MAX_VIZ_SAMPLES {
                let overflow = buf.len() - MAX_VIZ_SAMPLES;
                buf.drain(0..overflow);
            }
        }
    }

    pub fn get_visualizer_points(&self, points_count: usize) -> Vec<f32> {
        if let Ok(buf) = self.visualizer_samples.lock() {
            if buf.is_empty() || points_count == 0 {
                return vec![0.0; points_count];
            }
            let chunk_size = (buf.len() / points_count).max(1);
            let mut points = Vec::with_capacity(points_count);
            for i in 0..points_count {
                let start = i * chunk_size;
                let end = ((i + 1) * chunk_size).min(buf.len());
                if start >= buf.len() {
                    points.push(0.0);
                    continue;
                }
                let mut max_val = 0.0f32;
                for &s in &buf[start..end] {
                    let v = s.abs();
                    if v > max_val {
                        max_val = v;
                    }
                }
                points.push(max_val.min(1.0));
            }
            points
        } else {
            vec![0.0; points_count]
        }
    }

    pub fn get_visualizer_raw_samples(&self) -> Vec<f32> {
        if let Ok(buf) = self.visualizer_samples.lock() {
            buf.clone()
        } else {
            Vec::new()
        }
    }
}

impl Default for SharedAudioState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn create_ring_buffer(capacity_samples: usize) -> (Producer<f32>, Consumer<f32>, Arc<SharedAudioState>) {
    let (producer, consumer) = RingBuffer::new(capacity_samples);
    let state = Arc::new(SharedAudioState::new());
    (producer, consumer, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_and_atomic_state() {
        let (mut producer, mut consumer, state) = create_ring_buffer(1024);
        assert_eq!(state.get_volume(), 0.85);

        // 音量クランプ検証 (0.0〜1.0)
        state.set_volume(1.5);
        assert_eq!(state.get_volume(), 1.0);

        state.set_volume(-0.5);
        assert_eq!(state.get_volume(), 0.0);

        state.set_volume(0.65);
        assert_eq!(state.get_volume(), 0.65);

        // ミュート検証
        state.set_muted(true);
        assert!(state.is_muted());
        assert_eq!(state.get_volume(), 0.0);

        state.set_muted(false);
        assert!(!state.is_muted());
        assert_eq!(state.get_volume(), 0.65);

        // ロックフリープッシュとポップ
        let chunk = vec![0.1f32, 0.2, 0.3, 0.4];
        for &s in &chunk {
            assert!(producer.push(s).is_ok());
        }

        let mut read_buf = Vec::new();
        while let Ok(s) = consumer.pop() {
            read_buf.push(s);
        }
        assert_eq!(read_buf, chunk);

        // 空バッファからのpopはエラーを返す（非ブロッキング）
        assert!(consumer.pop().is_err());

        // 時間計算テスト
        state.sample_rate.store(44100, Ordering::Relaxed);
        state.current_sample_position.store(88200, Ordering::Relaxed);
        state.total_samples.store(441000, Ordering::Relaxed);
        assert_eq!(state.current_position_secs(), 2.0);
        assert_eq!(state.total_duration_secs(), 10.0);
    }
}

