use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub struct SharedAudioState {
    pub is_playing: AtomicBool,
    pub is_muted: AtomicBool,
    pub volume_bits: AtomicU32,
    pub current_sample_position: AtomicU64,
    pub total_samples: AtomicU64,
    pub sample_rate: AtomicU32,
    pub channels: AtomicU32,
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
