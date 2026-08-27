#[derive(Debug, Clone, PartialEq)]
pub struct AudioSignalStats {
    pub peak_amplitude: f32,
    pub rms: f32,
    pub has_clipping: bool,
    pub glitch_count: usize,
}

#[derive(Debug, Clone)]
pub struct WaveformAnalyzer {
    buffer: Vec<f32>,
    capacity: usize,
    write_idx: usize,
    count: usize,
}

impl WaveformAnalyzer {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64);
        Self {
            buffer: vec![0.0; cap],
            capacity: cap,
            write_idx: 0,
            count: 0,
        }
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        for &s in samples {
            self.buffer[self.write_idx] = s;
            self.write_idx = (self.write_idx + 1) % self.capacity;
            if self.count < self.capacity {
                self.count += 1;
            }
        }
    }

    /// 波形信号の健全性（ピーク、RMS、クリッピング、不連続グリッチ）を解析する
    pub fn analyze_signal(&self) -> AudioSignalStats {
        if self.count == 0 {
            return AudioSignalStats {
                peak_amplitude: 0.0,
                rms: 0.0,
                has_clipping: false,
                glitch_count: 0,
            };
        }

        let mut peak = 0.0f32;
        let mut sum_sq = 0.0f64;
        let mut has_clipping = false;
        let mut glitch_count = 0;

        let mut prev_sample = 0.0f32;
        let mut prev_valid = false;

        for i in 0..self.count {
            let idx = if self.count < self.capacity {
                i
            } else {
                (self.write_idx + i) % self.capacity
            };
            let s = self.buffer[idx];
            let abs_s = s.abs();

            if abs_s > peak {
                peak = abs_s;
            }
            if abs_s > 1.05 {
                has_clipping = true;
            }
            sum_sq += (s as f64) * (s as f64);

            // 1サンプル間での急峻な不連続変化（振幅差 > 1.2、または有音から突如0.0への断続ドロップ）の検知
            if prev_valid {
                let delta = (s - prev_sample).abs();
                if delta > 1.2 {
                    glitch_count += 1;
                }
            }
            prev_sample = s;
            prev_valid = true;
        }

        let rms = (sum_sq / self.count as f64).sqrt() as f32;

        AudioSignalStats {
            peak_amplitude: peak,
            rms,
            has_clipping,
            glitch_count,
        }
    }

    /// TUI描画用にダウンサンプリングされた波形データ点を取得（各区間のピークまたは平均）
    pub fn get_waveform_points(&self, points_count: usize) -> Vec<f32> {
        if self.count == 0 || points_count == 0 {
            return vec![0.0; points_count];
        }

        let mut points = Vec::with_capacity(points_count);
        let chunk_size = (self.count as f64 / points_count as f64).max(1.0);

        for i in 0..points_count {
            let start = (i as f64 * chunk_size) as usize;
            let end = (((i + 1) as f64 * chunk_size) as usize).min(self.count);

            if start >= end {
                points.push(0.0);
                continue;
            }

            let mut max_val = 0.0f32;
            for j in start..end {
                let idx = if self.count < self.capacity {
                    j
                } else {
                    (self.write_idx + j) % self.capacity
                };
                let val = self.buffer[idx].abs();
                if val > max_val {
                    max_val = val;
                }
            }
            points.push(max_val.min(1.0));
        }

        points
    }
}

impl Default for WaveformAnalyzer {
    fn default() -> Self {
        Self::new(2048)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_waveform_analyzer_clean_sine_wave() {
        let mut analyzer = WaveformAnalyzer::new(1024);

        // 綺麗な正弦波を生成して投入
        let mut sine_samples = Vec::new();
        for i in 0..1024 {
            let t = i as f32 / 44100.0;
            let sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.8;
            sine_samples.push(sample);
        }

        analyzer.push_samples(&sine_samples);
        let stats = analyzer.analyze_signal();

        assert!(stats.peak_amplitude >= 0.79 && stats.peak_amplitude <= 0.81);
        assert!(stats.rms >= 0.55 && stats.rms <= 0.60);
        assert!(!stats.has_clipping);
        assert_eq!(stats.glitch_count, 0); // 正常波形ではグリッチゼロ

        let points = analyzer.get_waveform_points(32);
        assert_eq!(points.len(), 32);
        assert!(points.iter().all(|&p| p >= 0.0 && p <= 1.0));
    }

    #[test]
    fn test_waveform_analyzer_detects_clipping_and_glitches() {
        let mut analyzer = WaveformAnalyzer::new(512);

        // 音割れ（>1.0）および激しい矩形波的グリッチ
        let mut glitchy_samples = Vec::new();
        for i in 0..512 {
            if i % 2 == 0 {
                glitchy_samples.push(1.5f32); // クリッピング
            } else {
                glitchy_samples.push(-1.5f32); // 急峻な断絶
            }
        }

        analyzer.push_samples(&glitchy_samples);
        let stats = analyzer.analyze_signal();

        assert!(stats.has_clipping);
        assert!(stats.glitch_count > 0);
    }
}
