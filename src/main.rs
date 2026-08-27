use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "myuujik", author, version, about = "High-quality, ultra-lightweight TUI audio player")]
struct CliArgs {
    /// Path to an audio file or directory to play
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    /// Override UI language ("ja" or "en")
    #[arg(short, long)]
    locale: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = CliArgs::parse();

    // 設定ファイルの読み込み
    let mut config = myuujik::config::AppConfig::load();
    if let Some(locale) = args.locale {
        config.ui.locale = locale;
    }

    // 多言語辞書の初期化
    let i18n = myuujik::i18n::I18n::new(PathBuf::from("locales"), &config.ui.locale)?;

    let volume_val = format!("{}", (config.audio.volume * 100.0) as u32);
    let volume_str = i18n.t_args("controls.volume", &[("val", &volume_val)]);

    println!("{} - {}", i18n.t("app.title"), i18n.t("app.subtitle"));
    println!("{}: {}", i18n.t("track_info.output_mode"), config.audio.output_mode);
    println!("{}", volume_str);

    if let Some(target_path) = args.path {
        println!("Target path: {:?}", target_path);
    } else if let Some(ref last_path) = config.session.last_opened_path {
        println!("Resuming last session: {}", last_path);
    }

    Ok(())
}

