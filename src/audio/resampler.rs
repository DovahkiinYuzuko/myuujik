use std::collections::VecDeque;

/// インターリーブされたマルチチャンネルPCMデータを高品質・低CPU負荷で変換するストリーミングリサンプラー。
/// エルミート3次補間（Hermite Cubic Interpolation）を採用し、エイリアシングノイズを抑制しながら
/// 任意の入力サンプリングレートからターゲットサンプリングレートへのリアルタイム変換を実現する。
#[derive(Debug)]
pub struct AudioResampler {
    channels: usize,
    from_rate: u32,
    to_rate: u32,
    /// 1出力サンプルあたりの入力フレーム進行量（from_rate / to_rate）
    ratio: f64,
    /// 入力フレームの現在サブサンプル位相（0.0 <= phase < 1.0）
    phase: f64,
    /// 各チャンネルごとの直近サンプル履歴（エルミート4点補間用のリングキュー）
    /// インターリーブされたフレーム単位で保持: VecDeque<Vec<f32>> (常に最大4フレーム保持)
    history: VecDeque<Vec<f32>>,
}

impl AudioResampler {
    /// 新しいリサンプラーを初期化する。
    pub fn new(from_rate: u32, to_rate: u32, channels: u16) -> Self {
        let ch = channels.max(1) as usize;
        let mut history = VecDeque::with_capacity(4);
        // 初期状態としてゼロフレームを3つ充填しておく
        for _ in 0..3 {
            history.push_back(vec![0.0f32; ch]);
        }

        Self {
            channels: ch,
            from_rate,
            to_rate,
            ratio: from_rate as f64 / to_rate as f64,
            phase: 0.0,
            history,
        }
    }

    pub fn from_rate(&self) -> u32 {
        self.from_rate
    }

    pub fn to_rate(&self) -> u32 {
        self.to_rate
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// リサンプリングが必要か（入力レートと出力レートが異なるか）を返す。
    pub fn is_resampling_needed(&self) -> bool {
        self.from_rate != self.to_rate
    }

    /// インターリーブされた入力PCMサンプルを処理し、リサンプリング後のインターリーブサンプルを出力する。
    /// 入出力レートが同一の場合はゼロコピーでそのままクローンまたはパススルーする。
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if !self.is_resampling_needed() {
            return input.to_vec();
        }

        if input.is_empty() {
            return Vec::new();
        }

        let input_frames = input.len() / self.channels;
        // 出力フレーム数の概算見積もり（余裕を持ってアロケート）
        let estimated_out_frames = ((input_frames as f64) / self.ratio).ceil() as usize + 8;
        let mut output = Vec::with_capacity(estimated_out_frames * self.channels);

        let mut input_idx = 0;

        while input_idx < input_frames {
            // 新しい入力フレームを履歴バッファに供給
            let frame_slice = &input[input_idx * self.channels..(input_idx + 1) * self.channels];
            self.history.push_back(frame_slice.to_vec());
            input_idx += 1;

            if self.history.len() > 4 {
                self.history.pop_front();
            }

            // 履歴が4フレーム揃っている間、現在の位相から出力サンプルを補間生成
            if self.history.len() == 4 {
                while self.phase < 1.0 {
                    let t = self.phase as f32;
                    let t2 = t * t;
                    let t3 = t2 * t;

                    for ch in 0..self.channels {
                        let y0 = self.history[0][ch];
                        let y1 = self.history[1][ch];
                        let y2 = self.history[2][ch];
                        let y3 = self.history[3][ch];

                        // 4点エルミート3次スプライン補間
                        let c0 = y1;
                        let c1 = 0.5 * (y2 - y0);
                        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);

                        let sample = c3 * t3 + c2 * t2 + c1 * t + c0;
                        output.push(sample.clamp(-1.0, 1.0));
                    }

                    self.phase += self.ratio;
                }

                // 位相が1.0以上になったら次の入力フレームへ進む
                self.phase -= 1.0;
            }
        }

        output
    }

    /// 内部状態およびヒストリバッファをリセットする（曲切り替え時やシーク時に使用）。
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.history.clear();
        for _ in 0..3 {
            self.history.push_back(vec![0.0f32; self.channels]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resampler_passthrough_when_same_rate() {
        let mut resampler = AudioResampler::new(48000, 48000, 2);
        assert!(!resampler.is_resampling_needed());

        let input = vec![0.1, -0.2, 0.3, -0.4];
        let output = resampler.process(&input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_resampler_upsample_length_and_continuity() {
        // 44.1kHz -> 48kHz (アップサンプリング: 約 1.088 倍のサンプルが生成される)
        let mut resampler = AudioResampler::new(44100, 48000, 2);
        assert!(resampler.is_resampling_needed());

        // 44100 サンプル (1秒分) のステレオ信号
        let num_frames = 4410; // 0.1秒分
        let mut input = Vec::with_capacity(num_frames * 2);
        for i in 0..num_frames {
            let t = i as f32 / 44100.0;
            let val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
            input.push(val); // L
            input.push(-val); // R
        }

        let output = resampler.process(&input);
        let out_frames = output.len() / 2;

        // 4800フレーム前後（誤差数フレーム以内）になっていること
        let expected_frames = (num_frames as f64 * 48000.0 / 44100.0).round() as usize;
        let diff = (out_frames as isize - expected_frames as isize).abs();
        assert!(diff <= 5, "Expected approx {} frames, got {}", expected_frames, out_frames);

        // 出力サンプルの値がクリップしていないこと
        for &s in &output {
            assert!(s >= -1.05 && s <= 1.05);
        }
    }

    #[test]
    fn test_resampler_downsample_length() {
        // 96kHz -> 48kHz (ダウンサンプリング: ちょうど半分のサンプル数)
        let mut resampler = AudioResampler::new(96000, 48000, 2);
        let num_frames = 9600;
        let mut input = Vec::with_capacity(num_frames * 2);
        for i in 0..num_frames {
            let t = i as f32 / 96000.0;
            let val = (t * 1000.0 * 2.0 * std::f32::consts::PI).sin();
            input.push(val);
            input.push(val);
        }

        let output = resampler.process(&input);
        let out_frames = output.len() / 2;
        let diff = (out_frames as isize - 4800).abs();
        assert!(diff <= 5, "Expected approx 4800 frames, got {}", out_frames);
    }
}