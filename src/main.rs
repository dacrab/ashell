use crate::config::{Position, get_config};
use crate::outputs::Outputs;
use app::App;
use clap::Parser;
use color_eyre::eyre::Context;
use iced::{Anchor, Font, KeyboardInteractivity, Layer, LayerShellSettings};
use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing::{debug, error};
use tracing_subscriber::layer::Layer as _;
use tracing_subscriber::{
    EnvFilter,
    filter::LevelFilter,
    layer::SubscriberExt,
    reload,
    util::SubscriberInitExt,
};

mod app;
mod components;
mod config;
mod i18n;
mod ipc;
mod modules;
mod osd;
mod outputs;
mod services;
mod theme;
mod utils;
mod xdg;

const NERD_FONT: &[u8] = include_bytes!("../target/generated/SymbolsNerdFont-Regular-Subset.ttf");
const NERD_FONT_MONO: &[u8] =
    include_bytes!("../target/generated/SymbolsNerdFontMono-Regular-Subset.ttf");
const CUSTOM_FONT: &[u8] = include_bytes!("../assets/AshellCustomIcon-Regular.otf");
const HEIGHT: f64 = 34.;
type LogSetter = Box<dyn Fn(&str) + Send + Sync>;
pub(crate) static SET_LOG_LEVEL: OnceLock<LogSetter> = OnceLock::new();

#[derive(Parser, Debug)]
#[command(
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
struct Args {
    #[arg(short, long, value_parser = clap::value_parser!(PathBuf))]
    config_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Send a message to a running ashell instance
    Msg {
        #[command(subcommand)]
        command: ipc::IpcCommand,
    },
}

fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    let (filter, reload_handle) = reload::Layer::new(EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy());

    SET_LOG_LEVEL
        .set(Box::new({
            let handle = reload_handle.clone();
            move |level_str: &str| {
                let new_filter = EnvFilter::builder()
                    .with_default_directive(
                        level_str
                            .parse::<LevelFilter>()
                            .unwrap_or(LevelFilter::INFO)
                            .into(),
                    )
                    .from_env_lossy();
                let _ = handle.reload(new_filter);
            }
        }))
        .ok();

    let logdir = xdg::get_runtime_dir()
        .unwrap_or_else(|| [env::temp_dir(), PathBuf::from("ashell")].iter().collect());
    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("ashell")
        .max_log_files(7)
        .build(logdir)
        .wrap_err("Failed to create log file appender")?;

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .with_filter(EnvFilter::builder()
                    .with_default_directive(LevelFilter::WARN.into())
                    .from_env_lossy()),
        )
        .init();

    let args = Args::parse();

    if let Some(Command::Msg { command }) = &args.command {
        if let Err(e) = ipc::run_client(command) {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    debug!("args: {args:?}");

    let (config, config_path) = get_config(args.config_path).unwrap_or_else(|err| {
        error!("Failed to read config: {err}");
        std::process::exit(1);
    });

    reload_handle.reload(
        EnvFilter::builder()
            .with_default_directive(
                config.log_level.parse::<LevelFilter>()
                    .unwrap_or(LevelFilter::INFO)
                    .into(),
            )
            .from_env_lossy(),
    ).wrap_err("Failed to set initial log level")?;

    let font = if let Some(font_name) = &config.appearance.font_name {
        Font::with_name(Box::leak(font_name.clone().into_boxed_str()))
    } else {
        Font::DEFAULT
    };

    let height = Outputs::get_height(config.appearance.style, config.appearance.scale_factor);

    let iced_layer = match config.layer {
        config::Layer::Top => Layer::Top,
        config::Layer::Bottom => Layer::Bottom,
        config::Layer::Overlay => Layer::Overlay,
    };

    let app = App::new((config.clone(), config_path));

    iced::application(
        app,
        App::update,
        App::view,
    )
    .layer_shell(LayerShellSettings {
        anchor: match config.position {
            Position::Top => Anchor::TOP,
            Position::Bottom => Anchor::BOTTOM,
        } | Anchor::LEFT
            | Anchor::RIGHT,
        layer: iced_layer,
        exclusive_zone: height as i32,
        size: Some((0, height as u32)),
        keyboard_interactivity: KeyboardInteractivity::None,
        namespace: "ashell-main-layer".into(),
        ..Default::default()
    })
    .subscription(App::subscription)
    .theme(App::theme)
    .scale_factor(App::scale_factor)
    .font(NERD_FONT)
    .font(NERD_FONT_MONO)
    .font(CUSTOM_FONT)
    .default_font(font)
    .run()?;

    Ok(())
}
