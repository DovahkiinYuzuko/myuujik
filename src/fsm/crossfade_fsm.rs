#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadeState {
    Normal,
    Crossfading { progress_frames: u64, total_frames: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossfadeEvent {
    StartCrossfade { total_frames: u64 },
    Advance { frames: u64 },
    Complete,
    Reset,
}

#[derive(Debug, Clone)]
pub struct CrossfadeFsm {
    state: CrossfadeState,
}

impl CrossfadeFsm {
    pub fn new() -> Self {
        Self {
            state: CrossfadeState::Normal,
        }
    }

    pub fn state(&self) -> &CrossfadeState {
        &self.state
    }

    pub fn is_crossfading(&self) -> bool {
        matches!(self.state, CrossfadeState::Crossfading { .. })
    }

    pub fn transition(&mut self, event: CrossfadeEvent) -> bool {
        match (self.state, event) {
            (CrossfadeState::Normal, CrossfadeEvent::StartCrossfade { total_frames }) => {
                if total_frames > 0 {
                    self.state = CrossfadeState::Crossfading {
                        progress_frames: 0,
                        total_frames,
                    };
                    true
                } else {
                    false
                }
            }
            (CrossfadeState::Crossfading { progress_frames, total_frames }, CrossfadeEvent::Advance { frames }) => {
                let new_progress = (progress_frames + frames).min(total_frames);
                self.state = CrossfadeState::Crossfading {
                    progress_frames: new_progress,
                    total_frames,
                };
                true
            }
            (CrossfadeState::Crossfading { .. }, CrossfadeEvent::Complete) => {
                self.state = CrossfadeState::Normal;
                true
            }
            (_, CrossfadeEvent::Reset) => {
                self.state = CrossfadeState::Normal;
                true
            }
            _ => false,
        }
    }

    pub fn is_finished(&self) -> bool {
        match self.state {
            CrossfadeState::Crossfading { progress_frames, total_frames } => progress_frames >= total_frames,
            CrossfadeState::Normal => false,
        }
    }

    /// イコールパワー則（二乗和が1.0）に基づく (fade_out_gain, fade_in_gain) を返す
    pub fn gains(&self) -> (f32, f32) {
        match self.state {
            CrossfadeState::Normal => (1.0, 0.0),
            CrossfadeState::Crossfading { progress_frames, total_frames } => {
                if total_frames == 0 {
                    return (1.0, 0.0);
                }
                let t = (progress_frames as f64 / total_frames as f64).clamp(0.0, 1.0);
                let angle = t * std::f64::consts::FRAC_PI_2;
                (angle.cos() as f32, angle.sin() as f32)
            }
        }
    }
}

impl Default for CrossfadeFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossfade_fsm_lifecycle() {
        let mut fsm = CrossfadeFsm::new();
        assert_eq!(fsm.state(), &CrossfadeState::Normal);
        assert_eq!(fsm.gains(), (1.0, 0.0));

        // 開始
        assert!(fsm.transition(CrossfadeEvent::StartCrossfade { total_frames: 1000 }));
        assert!(fsm.is_crossfading());

        // 進捗
        assert!(fsm.transition(CrossfadeEvent::Advance { frames: 500 }));
        if let CrossfadeState::Crossfading { progress_frames, total_frames } = fsm.state() {
            assert_eq!(*progress_frames, 500);
            assert_eq!(*total_frames, 1000);
        } else {
            panic!("Expected Crossfading state");
        }

        // 完了
        assert!(fsm.transition(CrossfadeEvent::Advance { frames: 500 }));
        assert!(fsm.is_finished());

        assert!(fsm.transition(CrossfadeEvent::Complete));
        assert_eq!(fsm.state(), &CrossfadeState::Normal);
        assert_eq!(fsm.gains(), (1.0, 0.0));
    }

    #[test]
    fn test_crossfade_fsm_equal_power_sum() {
        let mut fsm = CrossfadeFsm::new();
        fsm.transition(CrossfadeEvent::StartCrossfade { total_frames: 100 });

        for p in 0..=100 {
            fsm.state = CrossfadeState::Crossfading {
                progress_frames: p,
                total_frames: 100,
            };
            let (g_out, g_in) = fsm.gains();
            let sum_sq = g_out * g_out + g_in * g_in;
            assert!((sum_sq - 1.0).abs() < 1e-5, "Equal power sum should be 1.0 (got {})", sum_sq);
        }
    }

    #[test]
    fn test_crossfade_fsm_reset() {
        let mut fsm = CrossfadeFsm::new();
        fsm.transition(CrossfadeEvent::StartCrossfade { total_frames: 1000 });
        assert!(fsm.is_crossfading());

        assert!(fsm.transition(CrossfadeEvent::Reset));
        assert_eq!(fsm.state(), &CrossfadeState::Normal);
    }
}
