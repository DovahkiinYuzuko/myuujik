#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisualizerMode {
    #[default]
    Type3,       // AviUtl Type 3: 音量メーター (バーグラフ)
    Type4,       // AviUtl Type 4: 波状の音量メーター (波状補間曲線)
    Type3Polar,  // AviUtl Type 3 極座標変換: 円形サークル波形
}

impl VisualizerMode {
    pub fn next(self) -> Self {
        match self {
            VisualizerMode::Type3 => VisualizerMode::Type4,
            VisualizerMode::Type4 => VisualizerMode::Type3Polar,
            VisualizerMode::Type3Polar => VisualizerMode::Type3,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            VisualizerMode::Type3 => "METER",
            VisualizerMode::Type4 => "WAVE",
            VisualizerMode::Type3Polar => "CIRCLE",
        }
    }
}

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

    /// Type 3 極座標変換用に、中心座標 (cx, cy)、内径 r_inner、最大外径 r_max から
    /// 各放射状ラインの始点・終点座標のリストを算出する。
    pub fn compute_polar_lines(
        &self,
        cx: f64,
        cy: f64,
        r_inner: f64,
        r_max: f64,
        points_count: usize,
    ) -> Vec<((f64, f64), (f64, f64))> {
        let values = self.get_waveform_points(points_count);
        let mut lines = Vec::with_capacity(points_count);

        for (i, &val) in values.iter().enumerate() {
            let angle = (i as f64 / points_count as f64) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
            let r1 = r_inner;
            let r2 = r_inner + (val as f64) * (r_max - r_inner);

            let cos_a = angle.cos();
            let sin_a = angle.sin();

            let x1 = cx + cos_a * r1;
            let y1 = cy + sin_a * r1;
            let x2 = cx + cos_a * r2;
            let y2 = cy + sin_a * r2;

            lines.push(((x1, y1), (x2, y2)));
        }

        lines
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

    #[test]
    fn test_waveform_analyzer_compute_polar_lines() {
        let mut analyzer = WaveformAnalyzer::new(256);
        let samples = vec![0.5f32; 128];
        analyzer.push_samples(&samples);

        let lines = analyzer.compute_polar_lines(50.0, 50.0, 10.0, 30.0, 16);
        assert_eq!(lines.len(), 16);
        for ((x1, y1), (x2, y2)) in lines {
            // 内径半径が約10
            let r1 = ((x1 - 50.0).powi(2) + (y1 - 50.0).powi(2)).sqrt();
            assert!((r1 - 10.0).abs() < 1e-4);
            // 外径半径が10〜30の間
            let r2 = ((x2 - 50.0).powi(2) + (y2 - 50.0).powi(2)).sqrt();
            assert!(r2 >= 10.0 && r2 <= 30.001);
        }

        // モードの遷移テスト
        let mode = VisualizerMode::default();
        assert_eq!(mode, VisualizerMode::Type3);
        assert_eq!(mode.display_name(), "METER");
        let next_mode = mode.next();
        assert_eq!(next_mode, VisualizerMode::Type4);
        assert_eq!(next_mode.display_name(), "WAVE");
        let polar_mode = next_mode.next();
        assert_eq!(polar_mode, VisualizerMode::Type3Polar);
        assert_eq!(polar_mode.display_name(), "CIRCLE");
        assert_eq!(polar_mode.next(), VisualizerMode::Type3);
    }
}

