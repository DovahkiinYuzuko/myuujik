#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Buffering { track_id: usize, target_position_secs: f64 },
    Playing,
    Paused,
    Seeking { target_position_secs: f64 },
    TrackChanging { next_track_id: usize },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackEvent {
    Play(usize),
    Pause,
    Resume,
    TogglePause,
    Stop,
    Seek(f64),
    BufferReady,
    TrackFinished,
    DeviceError(String),
}

#[derive(Debug, Clone)]
pub struct PlaybackFsm {
    state: PlaybackState,
}

impl PlaybackFsm {
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
        }
    }

    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// 状態遷移を実行する。遷移が有効な場合は true、無効な場合は false を返す。
    pub fn transition(&mut self, event: PlaybackEvent) -> bool {
        let (new_state, valid) = match (&self.state, event) {
            // 任意状態からのデバイスエラー通知
            (_, PlaybackEvent::DeviceError(msg)) => {
                (PlaybackState::Error { message: msg }, true)
            }

            // Stopped からの遷移
            (PlaybackState::Stopped, PlaybackEvent::Play(id)) => {
                (PlaybackState::Buffering { track_id: id, target_position_secs: 0.0 }, true)
            }
            (PlaybackState::Stopped, _) => return false,

            // Buffering からの遷移
            (PlaybackState::Buffering { .. }, PlaybackEvent::BufferReady) => {
                (PlaybackState::Playing, true)
            }
            (PlaybackState::Buffering { .. }, PlaybackEvent::Pause | PlaybackEvent::TogglePause) => {
                (PlaybackState::Paused, true)
            }
            (PlaybackState::Buffering { .. }, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }
            (PlaybackState::Buffering { .. }, PlaybackEvent::Play(id)) => {
                (PlaybackState::Buffering { track_id: id, target_position_secs: 0.0 }, true)
            }

            // Playing からの遷移
            (PlaybackState::Playing, PlaybackEvent::Pause | PlaybackEvent::TogglePause) => {
                (PlaybackState::Paused, true)
            }
            (PlaybackState::Playing, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }
            (PlaybackState::Playing, PlaybackEvent::Seek(pos)) => {
                (PlaybackState::Seeking { target_position_secs: pos }, true)
            }
            (PlaybackState::Playing, PlaybackEvent::Play(id)) => {
                (PlaybackState::TrackChanging { next_track_id: id }, true)
            }
            (PlaybackState::Playing, PlaybackEvent::TrackFinished) => {
                (PlaybackState::Stopped, true)
            }

            // Paused からの遷移
            (PlaybackState::Paused, PlaybackEvent::Resume | PlaybackEvent::TogglePause) => {
                (PlaybackState::Playing, true)
            }
            (PlaybackState::Paused, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }
            (PlaybackState::Paused, PlaybackEvent::Seek(pos)) => {
                (PlaybackState::Seeking { target_position_secs: pos }, true)
            }
            (PlaybackState::Paused, PlaybackEvent::Play(id)) => {
                (PlaybackState::Buffering { track_id: id, target_position_secs: 0.0 }, true)
            }

            // Seeking からの遷移
            (PlaybackState::Seeking { .. }, PlaybackEvent::BufferReady) => {
                (PlaybackState::Playing, true)
            }
            (PlaybackState::Seeking { .. }, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }
            (PlaybackState::Seeking { .. }, PlaybackEvent::Seek(pos)) => {
                (PlaybackState::Seeking { target_position_secs: pos }, true)
            }

            // TrackChanging からの遷移
            (PlaybackState::TrackChanging { next_track_id: _ }, PlaybackEvent::BufferReady) => {
                (PlaybackState::Playing, true)
            }
            (PlaybackState::TrackChanging { .. }, PlaybackEvent::Play(id)) => {
                (PlaybackState::Buffering { track_id: id, target_position_secs: 0.0 }, true)
            }
            (PlaybackState::TrackChanging { .. }, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }

            // Error からの遷移
            (PlaybackState::Error { .. }, PlaybackEvent::Play(id)) => {
                (PlaybackState::Buffering { track_id: id, target_position_secs: 0.0 }, true)
            }
            (PlaybackState::Error { .. }, PlaybackEvent::Stop) => {
                (PlaybackState::Stopped, true)
            }

            // その他の無効な遷移
            _ => return false,
        };

        if valid {
            self.state = new_state;
        }
        valid
    }
}

impl Default for PlaybackFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_fsm_lifecycle() {
        let mut fsm = PlaybackFsm::new();
        assert_eq!(*fsm.state(), PlaybackState::Stopped);

        // 停止中の無効なイベント（シーク、一時停止、再開）は拒否
        assert!(!fsm.transition(PlaybackEvent::Seek(10.0)));
        assert!(!fsm.transition(PlaybackEvent::Pause));
        assert!(!fsm.transition(PlaybackEvent::Resume));
        assert!(!fsm.transition(PlaybackEvent::TogglePause));
        assert_eq!(*fsm.state(), PlaybackState::Stopped);

        // 再生開始 -> Buffering
        assert!(fsm.transition(PlaybackEvent::Play(0)));
        assert_eq!(*fsm.state(), PlaybackState::Buffering { track_id: 0, target_position_secs: 0.0 });

        // バッファ充填完了 -> Playing
        assert!(fsm.transition(PlaybackEvent::BufferReady));
        assert_eq!(*fsm.state(), PlaybackState::Playing);

        // 一時停止トグル -> Paused
        assert!(fsm.transition(PlaybackEvent::TogglePause));
        assert_eq!(*fsm.state(), PlaybackState::Paused);

        // 一時停止中のPauseイベントは無効
        assert!(!fsm.transition(PlaybackEvent::Pause));
        assert_eq!(*fsm.state(), PlaybackState::Paused);

        // 再開トグル -> Playing
        assert!(fsm.transition(PlaybackEvent::TogglePause));
        assert_eq!(*fsm.state(), PlaybackState::Playing);

        // 再生中のResumeイベントは無効
        assert!(!fsm.transition(PlaybackEvent::Resume));
        assert_eq!(*fsm.state(), PlaybackState::Playing);

        // シーク -> Seeking
        assert!(fsm.transition(PlaybackEvent::Seek(45.0)));
        assert_eq!(*fsm.state(), PlaybackState::Seeking { target_position_secs: 45.0 });

        // シーク完了 -> Playing
        assert!(fsm.transition(PlaybackEvent::BufferReady));
        assert_eq!(*fsm.state(), PlaybackState::Playing);

        // 曲変更 -> TrackChanging
        assert!(fsm.transition(PlaybackEvent::Play(2)));
        assert_eq!(*fsm.state(), PlaybackState::TrackChanging { next_track_id: 2 });

        // バッファ準備完了 -> Playing
        assert!(fsm.transition(PlaybackEvent::BufferReady));
        assert_eq!(*fsm.state(), PlaybackState::Playing);

        // トラック終了 -> Stopped
        assert!(fsm.transition(PlaybackEvent::TrackFinished));
        assert_eq!(*fsm.state(), PlaybackState::Stopped);

        // デバイスエラー発生 -> Error
        assert!(fsm.transition(PlaybackEvent::DeviceError("Device lost".to_string())));
        assert_eq!(*fsm.state(), PlaybackState::Error { message: "Device lost".to_string() });

        // エラー状態からの不正遷移ブロック（SeekやPauseは不可）
        assert!(!fsm.transition(PlaybackEvent::Seek(10.0)));
        assert!(!fsm.transition(PlaybackEvent::Pause));

        // エラーからの回復（Play） -> Buffering
        assert!(fsm.transition(PlaybackEvent::Play(1)));
        assert_eq!(*fsm.state(), PlaybackState::Buffering { track_id: 1, target_position_secs: 0.0 });
    }
}
