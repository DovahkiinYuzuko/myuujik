use clap::Parser;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "myuujik", author, version, about = "High-quality, ultra-lightweight TUI audio player")]
struct CliArgs {
    /// Path to an audio file or directory to play
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Override UI language ("ja" or "en")
    #[arg(short, long)]
    locale: Option<String>,

    /// Force exclusive output mode (Windows WASAPI)
    #[arg(short, long)]
    exclusive: bool,

    /// Enable shuffle mode
    #[arg(short, long)]
    shuffle: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = CliArgs::parse();

    // 設定ファイルの読み込み
    let mut config = myuujik::config::AppConfig::load();
    if let Some(locale) = args.locale {
        config.ui.locale = locale;
    }
    if args.exclusive {
        config.audio.output_mode = "Exclusive".to_string();
    }

    // 多言語辞書の初期化
    let i18n = myuujik::i18n::I18n::new(PathBuf::from("locales"), &config.ui.locale)?;

    let volume_val = format!("{}", (config.audio.volume * 100.0) as u32);
    let volume_str = i18n.t_args("controls.volume", &[("val", &volume_val)]);

    println!("{} - {}", i18n.t("app.title"), i18n.t("app.subtitle"));
    println!("{}: {}", i18n.t("track_info.output_mode"), config.audio.output_mode);
    println!("{}", volume_str);

    let target_path = args.path.or_else(|| {
        config.session.last_opened_path.as_ref().map(PathBuf::from)
    });

    if let Some(path) = target_path {
        let mut playlist = myuujik::playlist::PlaylistManager::new();
        if args.shuffle {
            playlist.set_shuffle(true);
        }

        let count = playlist.load_path(&path);
        let count_str = format!("{}", count);
        println!("{}", i18n.t_args("playlist.total_tracks", &[("count", &count_str)]));

        for item in playlist.items() {
            println!("  [{}] {}", item.id + 1, item.display_name);
        }

        if let Some(first_track) = playlist.select_and_play(0) {
            println!("\n▶ Opening: {}", first_track.display_name);
            let engine = myuujik::audio::AudioEngine::new(
                &config.audio.output_mode,
                &config.audio.output_device,
                config.audio.volume,
            )?;

            engine.play_file(&first_track.path);
            println!("Playback started. Active mode: {}", engine.active_output_mode());

            // 簡易再生ループ（進捗表示）
            let mut elapsed = 0;
            while elapsed < 40 {
                thread::sleep(Duration::from_millis(100));
                let cur = engine.current_position_secs();
                let dur = engine.total_duration_secs();
                let state = engine.current_state();
                print!("\r[State: {:?}] Progress: {:.1}s / {:.1}s    ", state, cur, dur);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                elapsed += 1;
            }
            println!("\nPlayback verified!");
        }
    }

    Ok(())
}
