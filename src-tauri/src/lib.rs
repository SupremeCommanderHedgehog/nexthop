// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

mod app_state;
mod commands;
mod config;
mod error;
mod prefs;
mod rate_limiter;
mod relay;
mod stats;
mod transport;

use app_state::{AppState, RelayState};
use clap::Parser;
use std::sync::Mutex;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

#[derive(clap::ValueEnum, Debug, Clone, Default)]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

/// Raw TCP/UDP relay with cross-protocol, multicast, and rate-limit support.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, default_value = "nexthop.toml")]
    config: String,

    /// Log output format.
    #[arg(long, value_enum, default_value_t = LogFormat::Text)]
    log_format: LogFormat,

    /// Disable the GUI and run in headless / command-line mode.
    #[arg(long)]
    no_gui: bool,
}

pub fn run() {
    let cli = Cli::parse();
    if cli.no_gui {
        run_headless(cli);
    } else {
        run_tauri(cli);
    }
}

fn run_headless(cli: Cli) {
    let cfg = match config::RelayConfig::from_file(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        }
    };

    let filter =
        EnvFilter::try_new(&cfg.general.log_level).unwrap_or_else(|_| EnvFilter::new("info"));

    match cli.log_format {
        LogFormat::Text => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(false)
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .init();
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let relay = relay::Relay::new(cfg, cli.config);
    if let Err(e) = rt.block_on(relay.run()) {
        tracing::error!(error = %e, "relay terminated with error");
        std::process::exit(1);
    }
}

fn run_tauri(cli: Cli) {
    let explicit_config = if cli.config == "nexthop.toml" {
        None
    } else {
        Some(std::path::PathBuf::from(&cli.config))
    };

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::start_relay,
            commands::stop_relay,
            commands::get_relay_status,
            commands::get_stats,
            commands::get_local_ips,
            commands::get_broadcast_ips,
            commands::get_prefs,
            commands::save_prefs,
        ])
        .setup(move |app| {
            let config_path = match explicit_config {
                Some(ref p) => p.clone(),
                None => {
                    let data_dir = app.path().app_data_dir()?;
                    std::fs::create_dir_all(&data_dir)?;
                    data_dir.join("nexthop.toml")
                }
            };
            let prefs_path = config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("preferences.toml");
            app.manage(AppState {
                relay: Mutex::new(RelayState::Stopped),
                config_path,
                prefs_path,
            });
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
