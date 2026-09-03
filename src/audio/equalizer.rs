use serde::{Deserialize, Serialize};

/// Robert Bristow-Johnson (RBJ) Audio EQ Cookbook に基づく Biquad IIR ピーキングフィルタ (Direct Form 2 Transposed)
#[derive(Debug, Clone)]
pub struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    s1: f32,
    s2: f32,
}

impl BiquadFilter {
    pub fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            s1: 0.0,
            s2: 0.0,
        }
    }

    /// ピーキングEQフィルタ係数を計算してリセットまたは更新
    pub fn update_peaking(&mut self, f0: f32, q: f32, gain_db: f32, sample_rate: f32) {
        // ナイキスト周波数を超える場合はパススルー（フラット）
        if f0 >= sample_rate * 0.49 || sample_rate <= 0.0 {
            self.b0 = 1.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            return;
        }

        // ゲインがほぼゼロの場合は計算を簡略化して完全パススルー
        if gain_db.abs() < 1e-4 {
            self.b0 = 1.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            return;
        }

        let a = 10.0f64.powf((gain_db as f64) / 40.0);
        let omega0 = 2.0 * std::f64::consts::PI * (f0 as f64) / (sample_rate as f64);
        let sin_omega = omega0.sin();
        let cos_omega = omega0.cos();
        let alpha = sin_omega / (2.0 * (q.max(0.1) as f64));

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha / a;

        let inv_a0 = 1.0 / a0;
        self.b0 = (b0 * inv_a0) as f32;
        self.b1 = (b1 * inv_a0) as f32;
        self.b2 = (b2 * inv_a0) as f32;
        self.a1 = (a1 * inv_a0) as f32;
        self.a2 = (a2 * inv_a0) as f32;
    }

    /// Direct Form 2 Transposed (DF2T) による 1 サンプル処理
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }

    /// ディレイ状態のクリア
    pub fn reset_state(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

impl Default for BiquadFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// 10バンド グラフィック・イコライザー（ISO標準中心周波数）
pub const EQ_BAND_COUNT: usize = 10;
pub const EQ_FREQUENCIES: [f32; EQ_BAND_COUNT] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const EQ_BAND_LABELS: [&str; EQ_BAND_COUNT] = [
    "31Hz", "62Hz", "125Hz", "250Hz", "500Hz", "1kHz", "2kHz", "4kHz", "8kHz", "16kHz",
];

/// EQジャンル別プリセット
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqPreset {
    Flat,
    BassBoost,
    Rock,
    Pop,
    Vocal,
    Jazz,
    Acoustic,
}

impl EqPreset {
    pub fn all() -> &'static [EqPreset] {
        &[
            EqPreset::Flat,
            EqPreset::BassBoost,
            EqPreset::Rock,
            EqPreset::Pop,
            EqPreset::Vocal,
            EqPreset::Jazz,
            EqPreset::Acoustic,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            EqPreset::Flat => "Flat",
            EqPreset::BassBoost => "Bass Boost",
            EqPreset::Rock => "Rock",
            EqPreset::Pop => "Pop",
            EqPreset::Vocal => "Vocal",
            EqPreset::Jazz => "Jazz",
            EqPreset::Acoustic => "Acoustic",
        }
    }

    pub fn gains(&self) -> [f32; EQ_BAND_COUNT] {
        match self {
            EqPreset::Flat => [0.0; EQ_BAND_COUNT],
            // Bass Boost: 低域を+5〜+7dB、中高域はフラット
            EqPreset::BassBoost => [7.0, 6.0, 5.0, 2.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            // Rock: 低域+4.5dB、中域-1.5dB、高域+4.0dBのドンシャリ
            EqPreset::Rock => [5.0, 3.5, 2.0, -0.5, -1.5, -0.5, 1.5, 3.0, 4.0, 4.5],
            // Pop: ボーカルとビートが聴きやすいマイルドカーブ
            EqPreset::Pop => [-1.0, 1.5, 3.0, 3.5, 2.0, 0.5, -0.5, -1.0, 1.5, 2.5],
            // Vocal: 声の主音域（500Hz〜4kHz）をブースト
            EqPreset::Vocal => [-2.0, -1.5, -0.5, 1.0, 3.5, 4.0, 3.5, 1.5, 0.0, -1.0],
            // Jazz: 温かみのあるベースと繊細なシンバル
            EqPreset::Jazz => [3.5, 2.5, 1.0, 1.5, -1.0, -1.0, 0.0, 1.5, 2.5, 3.0],
            // Acoustic: アコースティック弦楽器の空気感と抜け
            EqPreset::Acoustic => [3.0, 2.0, 1.0, 0.5, 1.0, 1.5, 2.5, 3.5, 3.0, 2.0],
        }
    }

    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "Flat" => Some(EqPreset::Flat),
            "Bass Boost" | "BassBoost" => Some(EqPreset::BassBoost),
            "Rock" => Some(EqPreset::Rock),
            "Pop" => Some(EqPreset::Pop),
            "Vocal" => Some(EqPreset::Vocal),
            "Jazz" => Some(EqPreset::Jazz),
            "Acoustic" => Some(EqPreset::Acoustic),
            _ => None,
        }
    }
}

/// 10バンド直列カスケードイコライザー（ステレオ/モノラル対応）
#[derive(Debug, Clone)]
pub struct Equalizer {
    pub enabled: bool,
    sample_rate: f32,
    gains: [f32; EQ_BAND_COUNT],
    filters_l: Vec<BiquadFilter>,
    filters_r: Vec<BiquadFilter>,
}

impl Equalizer {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        let mut filters_l = Vec::with_capacity(EQ_BAND_COUNT);
        let mut filters_r = Vec::with_capacity(EQ_BAND_COUNT);

        const Q: f32 = 1.414; // 1オクターブ帯域幅

        for &freq in &EQ_FREQUENCIES {
            let mut fl = BiquadFilter::new();
            fl.update_peaking(freq, Q, 0.0, sr);
            filters_l.push(fl);

            let mut fr = BiquadFilter::new();
            fr.update_peaking(freq, Q, 0.0, sr);
            filters_r.push(fr);
        }

        Self {
            enabled: true,
            sample_rate: sr,
            gains: [0.0; EQ_BAND_COUNT],
            filters_l,
            filters_r,
        }
    }

    pub fn gains(&self) -> &[f32; EQ_BAND_COUNT] {
        &self.gains
    }

    pub fn set_gain(&mut self, band_idx: usize, gain_db: f32) {
        if band_idx >= EQ_BAND_COUNT {
            return;
        }
        let clamped_gain = gain_db.clamp(-12.0, 12.0);
        self.gains[band_idx] = clamped_gain;

        const Q: f32 = 1.414;
        let freq = EQ_FREQUENCIES[band_idx];

        self.filters_l[band_idx].update_peaking(freq, Q, clamped_gain, self.sample_rate);
        self.filters_r[band_idx].update_peaking(freq, Q, clamped_gain, self.sample_rate);
    }

    pub fn set_gains(&mut self, gains: &[f32]) {
        for (i, &g) in gains.iter().take(EQ_BAND_COUNT).enumerate() {
            self.set_gain(i, g);
        }
    }

    pub fn apply_preset(&mut self, preset: EqPreset) {
        let preset_gains = preset.gains();
        self.set_gains(&preset_gains);
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(8000.0);
        if (self.sample_rate - sr).abs() < 1.0 {
            return;
        }
        self.sample_rate = sr;

        const Q: f32 = 1.414;
        for i in 0..EQ_BAND_COUNT {
            let freq = EQ_FREQUENCIES[i];
            let gain = self.gains[i];
            self.filters_l[i].update_peaking(freq, Q, gain, sr);
            self.filters_r[i].update_peaking(freq, Q, gain, sr);
        }
    }

    pub fn reset_state(&mut self) {
        for f in &mut self.filters_l {
            f.reset_state();
        }
        for f in &mut self.filters_r {
            f.reset_state();
        }
    }

    /// インターリーブされたステレオ/モノラルPCMサンプルスライスをインプレースでイコライジング
    pub fn process_interleaved(&mut self, samples: &mut [f32], channels: usize) {
        if !self.enabled || samples.is_empty() {
            return;
        }

        // 全バンドが 0.0dB（Flat）の場合はDSP処理を完全にバイパスしてCPU消費ゼロ
        let is_flat = self.gains.iter().all(|&g| g.abs() < 1e-3);
        if is_flat {
            return;
        }

        if channels >= 2 {
            for frame in samples.chunks_exact_mut(channels) {
                let mut l = frame[0];
                let mut r = frame[1];

                for i in 0..EQ_BAND_COUNT {
                    l = self.filters_l[i].process(l);
                    r = self.filters_r[i].process(r);
                }

                // デジタルクリッピング防止（ソフトサチュレーション / ハードクランプ）
                frame[0] = l.clamp(-1.0, 1.0);
                frame[1] = r.clamp(-1.0, 1.0);
            }
        } else if channels == 1 {
            for s in samples.iter_mut() {
                let mut val = *s;
                for i in 0..EQ_BAND_COUNT {
                    val = self.filters_l[i].process(val);
                }
                *s = val.clamp(-1.0, 1.0);
            }
        }
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biquad_filter_flat_response() {
        let mut filter = BiquadFilter::new();
        filter.update_peaking(1000.0, 1.414, 0.0, 44100.0);

        // 0.0dB ゲインのときはサンプルがそのまま通過すること
        for i in 0..100 {
            let sample = (i as f32 * 0.1).sin();
            let out = filter.process(sample);
            assert!((out - sample).abs() < 1e-4, "Sample should pass through cleanly at 0dB");
        }
    }

    #[test]
    fn test_equalizer_boost_increases_energy() {
        let mut eq = Equalizer::new(44100.0);
        // 1kHz (band 5) を +12dB ブースト
        eq.set_gain(5, 12.0);

        // 1kHzのサイン波を生成して投入
        let mut samples_flat = Vec::new();
        let mut samples_boosted = Vec::new();

        for i in 0..1000 {
            let t = i as f32 / 44100.0;
            let s = (t * 1000.0 * 2.0 * std::f32::consts::PI).sin() * 0.2;
            samples_flat.push(s);
            samples_flat.push(s); // ステレオ
            samples_boosted.push(s);
            samples_boosted.push(s);
        }

        eq.process_interleaved(&mut samples_boosted, 2);

        // 安定定常状態（後半）の実効値（RMS）を比較
        let rms_flat: f32 = (samples_flat[500..].iter().map(|&s| s * s).sum::<f32>() / 500.0).sqrt();
        let rms_boosted: f32 = (samples_boosted[500..].iter().map(|&s| s * s).sum::<f32>() / 500.0).sqrt();

        assert!(
            rms_boosted > rms_flat * 2.0,
            "+12dB boost should significantly increase RMS (flat: {}, boosted: {})",
            rms_flat,
            rms_boosted
        );
    }

    #[test]
    fn test_equalizer_sample_rate_adaptation() {
        let mut eq = Equalizer::new(44100.0);
        eq.apply_preset(EqPreset::Rock);

        // 96kHzにレート切り替え
        eq.set_sample_rate(96000.0);
        assert_eq!(eq.sample_rate, 96000.0);

        let mut samples = vec![0.5f32; 256];
        eq.process_interleaved(&mut samples, 2);

        // NaNや異常値が発生せず正常に動作すること
        for &s in &samples {
            assert!(!s.is_nan());
            assert!(s >= -1.0 && s <= 1.0);
        }
    }

    #[test]
    fn test_equalizer_preset_values() {
        let presets = EqPreset::all();
        assert_eq!(presets.len(), 7);

        for p in presets {
            let gains = p.gains();
            assert_eq!(gains.len(), EQ_BAND_COUNT);
            for &g in &gains {
                assert!(g >= -12.0 && g <= 12.0);
            }
        }
    }
}
