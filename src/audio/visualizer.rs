#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisualizerMode {
    #[default]
    Type3,    // AviUtl Type 3: 音量メーター (バーグラフ)
    Type4,    // AviUtl Type 4: 波状の音量メーター (波状補間曲線)
    Spectrum, // 本格FFT対数周波数スペクトラムアナライザ
}

impl VisualizerMode {
    pub fn next(self) -> Self {
        match self {
            VisualizerMode::Type3 => VisualizerMode::Type4,
            VisualizerMode::Type4 => VisualizerMode::Spectrum,
            VisualizerMode::Spectrum => VisualizerMode::Type3,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            VisualizerMode::Type3 => "METER",
            VisualizerMode::Type4 => "WAVE",
            VisualizerMode::Spectrum => "SPECTRUM",
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
}

impl Default for WaveformAnalyzer {
    fn default() -> Self {
        Self::new(2048)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Complex32 {
    re: f32,
    im: f32,
}

impl Complex32 {
    #[inline]
    fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            re: self.re + rhs.re,
            im: self.im + rhs.im,
        }
    }

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            re: self.re - rhs.re,
            im: self.im - rhs.im,
        }
    }

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }

    #[inline]
    fn norm_sq(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// In-place Cooley-Tukey Radix-2 FFT (Decimation-in-Time)
fn cooley_tukey_fft(data: &mut [Complex32]) {
    let n = data.len();
    if n <= 1 {
        return;
    }
    assert!(n.is_power_of_two(), "FFT size must be a power of two");

    // Bit-reversal permutation
    let mut j = 0;
    for i in 0..n {
        if i < j {
            data.swap(i, j);
        }
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
    }

    // Butterfly stages
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let angle = -2.0 * std::f32::consts::PI / (len as f32);
        let w_step = Complex32::new(angle.cos(), angle.sin());

        let mut i = 0;
        while i < n {
            let mut w = Complex32::new(1.0, 0.0);
            for k in 0..half {
                let u = data[i + k];
                let v = data[i + k + half].mul(w);
                data[i + k] = u.add(v);
                data[i + k + half] = u.sub(v);
                w = w.mul(w_step);
            }
            i += len;
        }
        len <<= 1;
    }
}

/// 本格FFT周波数スペクトラムアナライザ
#[derive(Debug, Clone)]
pub struct FftSpectrumAnalyzer {
    pub fft_size: usize,
    pub sample_rate: f32,
    window: Vec<f32>,
    complex_buffer: Vec<Complex32>,
    sample_ring: Vec<f32>,
    ring_write_idx: usize,
    sample_count: usize,
    bands_count: usize,
    band_ranges: Vec<(usize, usize)>, // 各バンドのFFTビン範囲 (start_bin, end_bin)
    band_tilts: Vec<f32>,            // +3dB/octave ピンクノイズ補正チルト倍率
    bar_heights: Vec<f32>,           // 現在のバーの高さ (0.0 .. 1.0)
    peak_heights: Vec<f32>,          // ピークドットの高さ (0.0 .. 1.0)
    decay_rate: f32,                 // バー下降の指数減衰率 (例: 0.85)
    peak_gravity: f32,               // ピーク落下の重力速度 (例: 0.035)
}

impl FftSpectrumAnalyzer {
    pub fn new(fft_size: usize, bands_count: usize, sample_rate: f32) -> Self {
        let n = if fft_size.is_power_of_two() {
            fft_size.max(256)
        } else {
            fft_size.next_power_of_two().max(256)
        };
        let bands = bands_count.clamp(8, 128);

        // 事前計算: ハン窓 (Hann Window)
        let mut window = Vec::with_capacity(n);
        for i in 0..n {
            let w = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos());
            window.push(w);
        }

        // 事前計算: 対数周波数ビニング & ピンクノイズ +3dB/oct チルト
        let f_min = 20.0f32;
        let f_max = (sample_rate * 0.5).min(20000.0);
        let bin_hz = sample_rate / n as f32;

        let mut band_ranges = Vec::with_capacity(bands);
        let mut band_tilts = Vec::with_capacity(bands);

        for k in 0..bands {
            let low_ratio = k as f32 / bands as f32;
            let high_ratio = (k + 1) as f32 / bands as f32;

            let f_low = f_min * (f_max / f_min).powf(low_ratio);
            let f_high = f_min * (f_max / f_min).powf(high_ratio);

            let start_bin = ((f_low / bin_hz).floor() as usize).clamp(1, n / 2 - 1);
            let end_bin = ((f_high / bin_hz).ceil() as usize).clamp(start_bin + 1, n / 2);

            band_ranges.push((start_bin, end_bin));

            // 中心周波数の計算と +3dB/octave チルトゲイン
            let f_center = (f_low * f_high).sqrt();
            let octaves = (f_center / f_min).log2().max(0.0);
            let tilt_db = octaves * 3.0; // +3.0 dB per octave
            let tilt_linear = 10.0f32.powf(tilt_db / 20.0);
            band_tilts.push(tilt_linear);
        }

        Self {
            fft_size: n,
            sample_rate,
            window,
            complex_buffer: vec![Complex32::default(); n],
            sample_ring: vec![0.0; n],
            ring_write_idx: 0,
            sample_count: 0,
            bands_count: bands,
            band_ranges,
            band_tilts,
            bar_heights: vec![0.0; bands],
            peak_heights: vec![0.0; bands],
            decay_rate: 0.85,
            peak_gravity: 0.035,
        }
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        for &s in samples {
            self.sample_ring[self.ring_write_idx] = s;
            self.ring_write_idx = (self.ring_write_idx + 1) % self.fft_size;
            if self.sample_count < self.fft_size {
                self.sample_count += 1;
            }
        }
    }

    pub fn process(&mut self) {
        if self.sample_count == 0 {
            for b in &mut self.bar_heights {
                *b = (*b * self.decay_rate).max(0.0);
            }
            for p in &mut self.peak_heights {
                *p = (*p - self.peak_gravity).max(0.0);
            }
            return;
        }

        // リングバッファから最新サンプルを取り出してハン窓を乗算
        let start_idx = if self.sample_count < self.fft_size {
            0
        } else {
            self.ring_write_idx
        };

        for i in 0..self.fft_size {
            let idx = (start_idx + i) % self.fft_size;
            let sample = self.sample_ring[idx] * self.window[i];
            self.complex_buffer[i] = Complex32::new(sample, 0.0);
        }

        // FFT変換
        cooley_tukey_fft(&mut self.complex_buffer);

        // 各対数周波数バンドのエネルギー集約 & dB正規化
        for k in 0..self.bands_count {
            let (start_bin, end_bin) = self.band_ranges[k];
            let mut sum_mag = 0.0f32;
            let count = (end_bin - start_bin).max(1);

            for bin in start_bin..end_bin {
                let mag_sq = self.complex_buffer[bin].norm_sq();
                sum_mag += mag_sq.sqrt();
            }
            let avg_mag = (sum_mag / count as f32) / (self.fft_size as f32 * 0.5);

            // チルト補正適用
            let tilted_mag = avg_mag * self.band_tilts[k];

            // dBスケーリング: 20 * log10(mag + 1e-6)
            // -60dB 〜 0dB を 0.0 〜 1.0 にマッピング
            let db = 20.0 * (tilted_mag.max(1e-6)).log10();
            let norm_val = ((db + 60.0) / 60.0).clamp(0.0, 1.0);

            // スムージング: アタックは即時、リリースは指数減衰
            let prev = self.bar_heights[k];
            let new_val = if norm_val > prev {
                norm_val
            } else {
                prev * self.decay_rate
            };
            self.bar_heights[k] = new_val;

            // ピークホールド更新
            let prev_peak = self.peak_heights[k];
            if new_val >= prev_peak {
                self.peak_heights[k] = new_val;
            } else {
                self.peak_heights[k] = (prev_peak - self.peak_gravity).max(new_val);
            }
        }
    }

    pub fn get_bands(&self) -> (&[f32], &[f32]) {
        (&self.bar_heights, &self.peak_heights)
    }
}

impl Default for FftSpectrumAnalyzer {
    fn default() -> Self {
        Self::new(1024, 64, 44100.0)
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
    fn test_visualizer_mode_transition() {
        let mode = VisualizerMode::default();
        assert_eq!(mode, VisualizerMode::Type3);
        assert_eq!(mode.display_name(), "METER");
        let next_mode = mode.next();
        assert_eq!(next_mode, VisualizerMode::Type4);
        assert_eq!(next_mode.display_name(), "WAVE");
        let next_mode2 = next_mode.next();
        assert_eq!(next_mode2, VisualizerMode::Spectrum);
        assert_eq!(next_mode2.display_name(), "SPECTRUM");
        assert_eq!(next_mode2.next(), VisualizerMode::Type3);
    }

    #[test]
    fn test_fft_spectrum_analyzer_detects_sine_peak() {
        let mut analyzer = FftSpectrumAnalyzer::new(1024, 64, 44100.0);

        // 440Hzのサイン波を1024サンプル生成
        let mut sine_samples = Vec::new();
        for i in 0..1024 {
            let t = i as f32 / 44100.0;
            let s = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.9;
            sine_samples.push(s);
        }

        analyzer.push_samples(&sine_samples);
        analyzer.process();

        let (bars, peaks) = analyzer.get_bands();
        assert_eq!(bars.len(), 64);
        assert_eq!(peaks.len(), 64);

        // 最大振幅を持つバンドのインデックスを探す
        let mut max_idx = 0;
        let mut max_val = 0.0f32;
        for (i, &val) in bars.iter().enumerate() {
            if val > max_val {
                max_val = val;
                max_idx = i;
            }
        }

        // 440Hzは20Hz〜20000Hzの対数64バンドの中盤（おおよそ25〜35バンド付近）にピークが来る
        assert!(max_idx >= 20 && max_idx <= 40, "Peak index was {}, expected near 440Hz", max_idx);
        assert!(max_val > 0.6, "Peak value was {}, expected > 0.6", max_val);
    }
}

