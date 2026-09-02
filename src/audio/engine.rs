use crate::audio::decoder::AudioDecoder;
use crate::audio::exclusive_wasapi::ExclusiveBackend;
use crate::audio::ring_buffer::SharedAudioState;
use crate::audio::shared::SharedBackend;
use crate::audio::traits::AudioOutputBackend;
use crate::fsm::playback_fsm::{PlaybackEvent, PlaybackFsm, PlaybackState};
use crate::logger;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

pub enum EngineCommand {
    PlayPath(PathBuf),
    Pause,
    Resume,
    TogglePause,
    Stop,
    Seek(f64),
    SetVolume(f32),
    SetOutputMode(String),
    SetOutputDevice(String),
}

#[derive(Clone)]
pub struct AudioEngine {
    cmd_tx: Sender<EngineCommand>,
    shared_state: Arc<SharedAudioState>,
    fsm: Arc<RwLock<PlaybackFsm>>,
    active_mode: Arc<RwLock<String>>,
    is_fallback: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn new(
        initial_mode: &str,
        initial_device: &str,
        initial_volume: f32,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let (cmd_tx, cmd_rx) = unbounded::<EngineCommand>();
        let shared_state = Arc::new(SharedAudioState::new());
        shared_state.set_volume(initial_volume);

        let fsm = Arc::new(RwLock::new(PlaybackFsm::new()));
        let active_mode = Arc::new(RwLock::new(initial_mode.to_string()));
        let is_fallback = Arc::new(AtomicBool::new(false));

        let shared_state_clone = Arc::clone(&shared_state);
        let fsm_clone = Arc::clone(&fsm);
        let active_mode_clone = Arc::clone(&active_mode);
        let is_fallback_clone = Arc::clone(&is_fallback);

        let initial_device_str = initial_device.to_string();
        let initial_mode_str = initial_mode.to_string();

        logger::info(
            "AudioEngine",
            &format!(
                "Initializing AudioEngine: mode={}, device={}, volume={:.2}",
                initial_mode_str, initial_device_str, initial_volume
            ),
        );

        thread::spawn(move || {
            #[cfg(windows)]
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            Self::worker_loop(
                cmd_rx,
                shared_state_clone,
                fsm_clone,
                active_mode_clone,
                is_fallback_clone,
                initial_mode_str,
                initial_device_str,
            );

            #[cfg(windows)]
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        });

        Ok(Self {
            cmd_tx,
            shared_state,
            fsm,
            active_mode,
            is_fallback,
        })
    }

    pub fn get_waveform_points(&self, points_count: usize) -> Vec<f32> {
        self.shared_state.get_visualizer_points(points_count)
    }

    pub fn send_command(&self, cmd: EngineCommand) -> Result<(), crossbeam_channel::SendError<EngineCommand>> {
        self.cmd_tx.send(cmd)
    }

    pub fn play_file<P: AsRef<Path>>(&self, path: P) {
        let _ = self.send_command(EngineCommand::PlayPath(path.as_ref().to_path_buf()));
    }

    pub fn pause(&self) {
        let _ = self.send_command(EngineCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.send_command(EngineCommand::Resume);
    }

    pub fn toggle_pause(&self) {
        let _ = self.send_command(EngineCommand::TogglePause);
    }

    pub fn stop(&self) {
        let _ = self.send_command(EngineCommand::Stop);
    }

    pub fn seek(&self, target_secs: f64) {
        let _ = self.send_command(EngineCommand::Seek(target_secs));
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared_state.set_volume(volume);
        let _ = self.send_command(EngineCommand::SetVolume(volume));
    }

    pub fn set_output_mode(&self, mode: &str) {
        let _ = self.send_command(EngineCommand::SetOutputMode(mode.to_string()));
    }

    pub fn current_state(&self) -> PlaybackState {
        self.fsm.read().unwrap().state().clone()
    }

    pub fn current_position_secs(&self) -> f64 {
        self.shared_state.current_position_secs()
    }

    pub fn total_duration_secs(&self) -> f64 {
        self.shared_state.total_duration_secs()
    }

    pub fn volume(&self) -> f32 {
        self.shared_state.get_volume()
    }

    pub fn active_output_mode(&self) -> String {
        self.active_mode.read().unwrap().clone()
    }

    pub fn is_fallback(&self) -> bool {
        self.is_fallback.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn set_total_duration_for_test(&self, duration_secs: f64) {
        self.shared_state.sample_rate.store(48000, Ordering::Relaxed);
        let frames = (duration_secs * 48000.0) as u64;
        self.shared_state.total_samples.store(frames, Ordering::Relaxed);
    }

    fn worker_loop(
        cmd_rx: Receiver<EngineCommand>,
        shared_state: Arc<SharedAudioState>,
        fsm: Arc<RwLock<PlaybackFsm>>,
        active_mode: Arc<RwLock<String>>,
        is_fallback: Arc<AtomicBool>,
        mut current_mode: String,
        mut current_device: String,
    ) {
        let mut decoder: Option<AudioDecoder> = None;
        let mut backend: Option<Box<dyn AudioOutputBackend>> = None;
        let mut producer: Option<rtrb::Producer<f32>> = None;
        let mut pending_samples: Vec<f32> = Vec::new();
        let mut pending_offset: usize = 0;

        const RING_BUFFER_SIZE: usize = 96_000; // 約1秒分 (48kHz Stereo)

        loop {
            // コマンド処理
            let mut is_disconnected = false;
            loop {
                let cmd = match cmd_rx.try_recv() {
                    Ok(c) => c,
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        is_disconnected = true;
                        break;
                    }
                };

                match cmd {
                    EngineCommand::PlayPath(path) => {
                        logger::info("AudioEngine", &format!("PlayPath command received: {:?}", path));
                        fsm.write().unwrap().transition(PlaybackEvent::Play(0));
                        shared_state.is_playing.store(false, Ordering::Relaxed);
                        shared_state.current_sample_position.store(0, Ordering::Relaxed);

                        // 旧バックエンドの排他ロック（WASAPI Exclusive等）およびリングバッファを先行解放
                        backend = None;
                        producer = None;
                        pending_samples.clear();
                        pending_offset = 0;
                        decoder = None;

                        match AudioDecoder::open(&path) {
                            Ok(dec) => {
                                let meta = dec.metadata().clone();
                                logger::info(
                                    "AudioEngine",
                                    &format!(
                                        "Decoded track info: codec={}, rate={}Hz, channels={}, bits={:?}, duration={:?}s",
                                        meta.codec_name,
                                        meta.sample_rate,
                                        meta.channels,
                                        meta.bits_per_sample,
                                        meta.duration_secs
                                    ),
                                );

                                shared_state.sample_rate.store(meta.sample_rate, Ordering::Relaxed);
                                shared_state.channels.store(meta.channels as u32, Ordering::Relaxed);

                                if let Some(dur) = meta.duration_secs {
                                    let total_frames = (dur * meta.sample_rate as f64) as u64;
                                    shared_state.total_samples.store(total_frames, Ordering::Relaxed);
                                }

                                // SPSCリングバッファの構築
                                let (new_prod, new_cons) = rtrb::RingBuffer::new(RING_BUFFER_SIZE);

                                // バックエンドの構築
                                let b_res = Self::init_backend(
                                    &current_mode,
                                    &current_device,
                                    meta.sample_rate,
                                    meta.channels,
                                    meta.bits_per_sample.unwrap_or(16) as u16,
                                    new_cons,
                                    Arc::clone(&shared_state),
                                    &is_fallback,
                                    &active_mode,
                                );

                                match b_res {
                                    Ok((mut b, used_prod)) => {
                                        let _ = b.play();
                                        backend = Some(b);
                                        producer = Some(used_prod.unwrap_or(new_prod));
                                        decoder = Some(dec);
                                        shared_state.is_playing.store(true, Ordering::Relaxed);
                                        fsm.write().unwrap().transition(PlaybackEvent::BufferReady);
                                        logger::info("AudioEngine", "Backend initialized and started successfully.");
                                    }
                                    Err(e) => {
                                        logger::error("AudioEngine", &format!("Backend initialization failed: {}", e));
                                        fsm.write().unwrap().transition(PlaybackEvent::DeviceError(e.to_string()));
                                    }
                                }
                            }
                            Err(e) => {
                                logger::error("AudioEngine", &format!("Failed to open audio file {:?}: {}", path, e));
                                fsm.write().unwrap().transition(PlaybackEvent::DeviceError(e.to_string()));
                            }
                        }
                    }
                    EngineCommand::Pause => {
                        logger::info("AudioEngine", "Pause command");
                        if fsm.write().unwrap().transition(PlaybackEvent::Pause) {
                            shared_state.is_playing.store(false, Ordering::Relaxed);
                        }
                    }
                    EngineCommand::Resume => {
                        logger::info("AudioEngine", "Resume command");
                        if fsm.write().unwrap().transition(PlaybackEvent::Resume) {
                            shared_state.is_playing.store(true, Ordering::Relaxed);
                        }
                    }
                    EngineCommand::TogglePause => {
                        let is_currently_playing = shared_state.is_playing.load(Ordering::Relaxed);
                        if is_currently_playing {
                            if fsm.write().unwrap().transition(PlaybackEvent::Pause) {
                                shared_state.is_playing.store(false, Ordering::Relaxed);
                            }
                        } else {
                            if fsm.write().unwrap().transition(PlaybackEvent::Resume) {
                                shared_state.is_playing.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    EngineCommand::Stop => {
                        logger::info("AudioEngine", "Stop command");
                        fsm.write().unwrap().transition(PlaybackEvent::Stop);
                        shared_state.is_playing.store(false, Ordering::Relaxed);
                        shared_state.current_sample_position.store(0, Ordering::Relaxed);
                        if let Some(mut b) = backend.take() {
                            let _ = b.pause();
                            drop(b);
                        }
                        backend = None;
                        producer = None;
                        decoder = None;
                        pending_samples.clear();
                        pending_offset = 0;
                    }
                    EngineCommand::Seek(target_secs) => {
                        logger::info("AudioEngine", &format!("Seek to {:.2}s", target_secs));
                        if fsm.write().unwrap().transition(PlaybackEvent::Seek(target_secs)) {
                            shared_state.is_playing.store(false, Ordering::Relaxed);
                            if let Some(dec) = decoder.as_mut() {
                                match dec.seek(target_secs) {
                                    Ok(actual) => {
                                        let rate = shared_state.sample_rate.load(Ordering::Relaxed).max(1);
                                        let sample_pos = (actual * rate as f64) as u64;
                                        shared_state.current_sample_position.store(sample_pos, Ordering::Relaxed);
                                        shared_state.seek_trigger.store(true, Ordering::Release);
                                        pending_samples.clear();
                                        pending_offset = 0;
                                        shared_state.is_playing.store(true, Ordering::Relaxed);
                                        fsm.write().unwrap().transition(PlaybackEvent::BufferReady);
                                    }
                                    Err(e) => {
                                        logger::error("AudioEngine", &format!("Seek failed: {}", e));
                                        shared_state.is_playing.store(true, Ordering::Relaxed);
                                        fsm.write().unwrap().transition(PlaybackEvent::BufferReady);
                                    }
                                }
                            } else {
                                shared_state.is_playing.store(true, Ordering::Relaxed);
                                fsm.write().unwrap().transition(PlaybackEvent::BufferReady);
                            }
                        }
                    }
                    EngineCommand::SetVolume(vol) => {
                        logger::debug("AudioEngine", &format!("Volume set to {:.2}", vol));
                        shared_state.set_volume(vol);
                    }
                    EngineCommand::SetOutputMode(mode) => {
                        logger::info("AudioEngine", &format!("Switching output mode from {} to {}", current_mode, mode));
                        current_mode = mode.clone();

                        // 再生中の場合はストリームを安全に再構築
                        if let Some(dec) = decoder.as_mut() {
                            let saved_secs = shared_state.current_position_secs();
                            let was_playing = shared_state.is_playing.load(Ordering::Relaxed);
                            shared_state.is_playing.store(false, Ordering::Relaxed);

                            if let Some(mut old_b) = backend.take() {
                                let _ = old_b.pause();
                                drop(old_b);
                            }
                            producer = None;

                            // WASAPI Exclusiveデバイス解放とCOMスレッド終了の安全マージン
                            std::thread::sleep(std::time::Duration::from_millis(25));

                            let meta = dec.metadata().clone();
                            let (new_prod, new_cons) = rtrb::RingBuffer::new(RING_BUFFER_SIZE);

                            let b_res = Self::init_backend(
                                &current_mode,
                                &current_device,
                                meta.sample_rate,
                                meta.channels,
                                meta.bits_per_sample.unwrap_or(16) as u16,
                                new_cons,
                                Arc::clone(&shared_state),
                                &is_fallback,
                                &active_mode,
                            );

                            match b_res {
                                Ok((mut b, used_prod)) => {
                                    let mut effective_prod = used_prod.unwrap_or(new_prod);

                                    // 再生位置の正確なシーク復帰（バッファ未消費による音声スキップを解消）
                                    if let Ok(actual) = dec.seek(saved_secs) {
                                        let rate = shared_state.sample_rate.load(Ordering::Relaxed).max(1);
                                        let sample_pos = (actual * rate as f64) as u64;
                                        shared_state.current_sample_position.store(sample_pos, Ordering::Relaxed);
                                        shared_state.seek_trigger.store(true, Ordering::Release);
                                    }

                                    // プリロール充填（新ストリーム開始時の即時アンダーラン防止）
                                    if let Ok(Some(samples)) = dec.next_interleaved_packet() {
                                        for s in samples {
                                            if effective_prod.push(s).is_err() {
                                                break;
                                            }
                                        }
                                    }

                                    if was_playing {
                                        let _ = b.play();
                                        shared_state.is_playing.store(true, Ordering::Relaxed);
                                    }

                                    backend = Some(b);
                                    producer = Some(effective_prod);
                                    logger::info(
                                        "AudioEngine",
                                        &format!(
                                            "Switched backend successfully to mode: {} (active: {}, fallback: {}) at {:.2}s",
                                            current_mode,
                                            *active_mode.read().unwrap(),
                                            is_fallback.load(Ordering::Relaxed),
                                            saved_secs
                                        ),
                                    );
                                }
                                Err(e) => {
                                    logger::error("AudioEngine", &format!("Failed to switch mode: {}", e));
                                    fsm.write().unwrap().transition(PlaybackEvent::DeviceError(e.to_string()));
                                }
                            }
                        } else {
                            // 停止中の場合もアクティブモード名を反映
                            *active_mode.write().unwrap() = current_mode.clone();
                            is_fallback.store(false, Ordering::Relaxed);
                        }
                    }
                    EngineCommand::SetOutputDevice(dev) => {
                        logger::info("AudioEngine", &format!("Setting output device to: {}", dev));
                        current_device = dev;
                    }
                }
            }

            if is_disconnected {
                logger::info("AudioEngine", "Command channel disconnected. Exiting worker thread cleanly.");
                break;
            }

            // デコードループ：残余サンプルのフラッシュと新規パケットのデコード（業界標準 Backpressure アーキテクチャ）
            if let (Some(dec), Some(prod)) = (decoder.as_mut(), producer.as_mut()) {
                let is_playing = shared_state.is_playing.load(Ordering::Relaxed);
                if is_playing {
                    let mut iterations = 0;
                    // リングバッファに空きがある限り、残余サンプルを注入し、必要に応じて次パケットをデコード
                    while prod.slots() > 0 && iterations < 16 {
                        iterations += 1;

                        // 1. 前回入り切らなかった残余サンプル（pending_samples）があれば、最優先で push
                        if pending_offset < pending_samples.len() {
                            let available_slots = prod.slots();
                            let remaining = pending_samples.len() - pending_offset;
                            let to_push = available_slots.min(remaining);

                            for &s in &pending_samples[pending_offset..pending_offset + to_push] {
                                let _ = prod.push(s);
                            }
                            pending_offset += to_push;

                            if pending_offset < pending_samples.len() {
                                // リングバッファが満杯になったため、残りは次回ループに持ち越し
                                break;
                            } else {
                                // 残余サンプルを全て push 完了
                                pending_samples.clear();
                                pending_offset = 0;
                            }
                        }

                        // 2. 残余サンプルが空になり、かつリングバッファにまだ空きがあれば、次のパケットを取得
                        if prod.slots() > 0 {
                            match dec.next_interleaved_packet() {
                                Ok(Some(samples)) => {
                                    if samples.is_empty() {
                                        continue;
                                    }
                                    let available_slots = prod.slots();
                                    let to_push = available_slots.min(samples.len());

                                    for &s in &samples[..to_push] {
                                        let _ = prod.push(s);
                                    }

                                    // 入り切らなかった残りは pending_samples に退避（1サンプルもドロップさせない）
                                    if to_push < samples.len() {
                                        pending_samples = samples;
                                        pending_offset = to_push;
                                        break; // バッファ満杯のためバックプレッシャー
                                    }
                                }
                                Ok(None) => {
                                    // トラック終了（残余サンプルも全てリングバッファに push 済み）
                                    logger::info("AudioEngine", "Track finished naturally.");
                                    fsm.write().unwrap().transition(PlaybackEvent::TrackFinished);
                                    shared_state.is_playing.store(false, Ordering::Relaxed);
                                    backend = None;
                                    producer = None;
                                    decoder = None;
                                    pending_samples.clear();
                                    pending_offset = 0;
                                    break;
                                }
                                Err(e) => {
                                    logger::error("AudioEngine", &format!("Decoding packet error: {}", e));
                                    fsm.write().unwrap().transition(PlaybackEvent::DeviceError(e.to_string()));
                                    pending_samples.clear();
                                    pending_offset = 0;
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(5));
        }
    }

    fn init_backend(
        mode: &str,
        device_name: &str,
        sample_rate: u32,
        channels: u16,
        bits_per_sample: u16,
        consumer: rtrb::Consumer<f32>,
        state: Arc<SharedAudioState>,
        is_fallback: &Arc<AtomicBool>,
        active_mode: &Arc<RwLock<String>>,
    ) -> Result<(Box<dyn AudioOutputBackend>, Option<rtrb::Producer<f32>>), Box<dyn Error + Send + Sync>> {
        logger::info(
            "AudioEngine",
            &format!(
                "init_backend request: mode={}, device={}, rate={}, ch={}, bits={}",
                mode, device_name, sample_rate, channels, bits_per_sample
            ),
        );

        if mode == "Mock" {
            is_fallback.store(false, Ordering::Relaxed);
            *active_mode.write().unwrap() = "Mock (In-Memory)".to_string();
            let mock = crate::audio::mock::MockAudioBackend::create(consumer, state);
            logger::info("AudioEngine", "MockAudioBackend created successfully.");
            return Ok((Box::new(mock), None));
        }

        if mode == "Exclusive" && ExclusiveBackend::is_supported() {
            let res = ExclusiveBackend::create(
                device_name,
                sample_rate,
                channels,
                bits_per_sample,
                consumer,
                Arc::clone(&state),
            );

            match res {
                Ok(b) => {
                    logger::info("AudioEngine", "ExclusiveBackend created and verified.");
                    is_fallback.store(false, Ordering::Relaxed);
                    *active_mode.write().unwrap() = "Exclusive (Bit-Perfect)".to_string();
                    return Ok((Box::new(b), None));
                }
                Err(e) => {
                    logger::warn("AudioEngine", &format!("ExclusiveBackend failed: {}. Falling back to Shared mode.", e));
                    // Shared Mode へ自動フォールバック（新リングバッファを生成して接続）
                    is_fallback.store(true, Ordering::Relaxed);
                    *active_mode.write().unwrap() = "Shared (Fallback)".to_string();
                    let (p_fb, c_fb) = rtrb::RingBuffer::new(96_000);
                    let shared = SharedBackend::create(device_name, sample_rate, channels, c_fb, state)?;
                    return Ok((Box::new(shared), Some(p_fb)));
                }
            }
        }

        // SharedBackend の構築
        is_fallback.store(false, Ordering::Relaxed);
        *active_mode.write().unwrap() = "Shared".to_string();
        let shared = SharedBackend::create(device_name, sample_rate, channels, consumer, state)?;
        logger::info("AudioEngine", "SharedBackend created successfully.");
        Ok((Box::new(shared), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_audio_engine_lifecycle_and_commands() {
        let engine = AudioEngine::new("Mock", "Default", 0.85).expect("failed to init engine");
        assert_eq!(engine.volume(), 0.85);

        engine.set_volume(0.5);
        assert_eq!(engine.volume(), 0.5);

        let sample_wav = Path::new("sample/Kendrick Lamar - Not Like Us.wav");
        if sample_wav.exists() {
            engine.play_file(sample_wav);
            // デコード開始・Playing遷移待機
            let mut waited = 0;
            while waited < 50 && engine.current_state() != PlaybackState::Playing && !matches!(engine.current_state(), PlaybackState::Error { .. }) {
                thread::sleep(Duration::from_millis(20));
                waited += 1;
            }

            if engine.current_state() == PlaybackState::Playing {
                // 一時停止と再開
                engine.pause();
                thread::sleep(Duration::from_millis(30));
                assert_eq!(engine.current_state(), PlaybackState::Paused);

                engine.resume();
                thread::sleep(Duration::from_millis(30));
                assert_eq!(engine.current_state(), PlaybackState::Playing);

                // シーク
                engine.seek(1.5);
                thread::sleep(Duration::from_millis(30));

                // 停止
                engine.stop();
                let mut waited = 0;
                while waited < 50 && engine.current_state() != PlaybackState::Stopped {
                    thread::sleep(Duration::from_millis(10));
                    waited += 1;
                }
                assert_eq!(engine.current_state(), PlaybackState::Stopped);
            }
        }
    }

    #[test]
    fn test_audio_engine_track_change_drops_old_backend() {
        let engine = AudioEngine::new("Mock", "Default", 0.85).expect("failed to init engine");
        let sample_wav = Path::new("sample/Kendrick Lamar - Not Like Us.wav");
        let sample_mp3 = Path::new("sample/Coolio - Gangsta's Paradise (feat. L.V.) [Official Music Video].mp3");
        if sample_wav.exists() && sample_mp3.exists() {
            // トラック1再生
            engine.play_file(sample_wav);
            thread::sleep(Duration::from_millis(50));
            // トラック2に即座に切り替え（旧バックエンドが即座にドロップされること）
            engine.play_file(sample_mp3);
            thread::sleep(Duration::from_millis(50));
            engine.stop();
            let mut waited = 0;
            while waited < 50 && engine.current_state() != PlaybackState::Stopped {
                thread::sleep(Duration::from_millis(10));
                waited += 1;
            }
            assert_eq!(engine.current_state(), PlaybackState::Stopped);
        }
    }

    #[test]
    fn test_audio_engine_output_mode_switch_during_playback() {
        let engine = AudioEngine::new("Mock", "Default", 0.85).expect("failed to init engine");
        let sample_wav = Path::new("sample/Kendrick Lamar - Not Like Us.wav");
        if sample_wav.exists() {
            engine.play_file(sample_wav);

            // 再生開始待機
            let mut waited = 0;
            while waited < 50 && engine.current_state() != PlaybackState::Playing && !matches!(engine.current_state(), PlaybackState::Error { .. }) {
                thread::sleep(Duration::from_millis(20));
                waited += 1;
            }

            if engine.current_state() == PlaybackState::Playing {
                // 再生中にモード切り替え
                engine.set_output_mode("Mock");
                thread::sleep(Duration::from_millis(100));
                assert_eq!(engine.current_state(), PlaybackState::Playing);

                engine.stop();
                let mut waited = 0;
                while waited < 50 && engine.current_state() != PlaybackState::Stopped {
                    thread::sleep(Duration::from_millis(10));
                    waited += 1;
                }
                assert_eq!(engine.current_state(), PlaybackState::Stopped);
            }
        }
    }
}

