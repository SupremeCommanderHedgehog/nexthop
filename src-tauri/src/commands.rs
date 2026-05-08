// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present nexthop@krypte.me

use crate::app_state::{AppState, RelayState};
use crate::config::{self, CastMode, DestConfig, EndpointConfig, EndpointMode, GeneralConfig,
                    OverflowPolicy, Protocol, RelayConfig};
use crate::prefs::Prefs;
use crate::relay::Relay;
use crate::stats::{Stats, StatsSnapshot};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;

// ── Default config returned when nexthop.toml does not exist yet ──

fn default_config() -> RelayConfig {
    RelayConfig {
        general: GeneralConfig {
            log_level: "info".into(),
            stats_interval_secs: 30,
            channel_capacity: 1024,
            max_payload_size: 65535,
            health_port: None,
        },
        source: EndpointConfig {
            name: None,
            protocol: Protocol::Udp,
            mode: EndpointMode::Server,
            address: "0.0.0.0:5000".into(),
            cast_mode: CastMode::Unicast,
            multicast_interface: None,
            multicast_interface_index: None,
            multicast_ttl: 16,
            reconnect_delay_ms: None,
        },
        rate_limit: None,
        destinations: vec![DestConfig {
            base: EndpointConfig {
                name: None,
                protocol: Protocol::Udp,
                mode: EndpointMode::Client,
                address: "127.0.0.1:5001".into(),
                cast_mode: CastMode::Unicast,
                multicast_interface: None,
                multicast_interface_index: None,
                multicast_ttl: 16,
                reconnect_delay_ms: None,
            },
            overflow_policy: OverflowPolicy::DropNewest,
        }],
    }
}

// ── get_config ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_config(state: State<AppState>) -> Result<RelayConfig, String> {
    let path = state
        .config_path
        .to_str()
        .ok_or("invalid config path")?;
    if !std::path::Path::new(path).exists() {
        return Ok(default_config());
    }
    config::RelayConfig::from_file(path).map_err(|e| e.to_string())
}

// ── save_config ───────────────────────────────────────────────────────

#[tauri::command]
pub fn save_config(config: RelayConfig, state: State<AppState>) -> Result<(), String> {
    config.validate().map_err(|e| e.to_string())?;
    let toml = config::to_toml_string(&config);
    std::fs::write(&state.config_path, toml).map_err(|e| e.to_string())
}

// ── start_relay ───────────────────────────────────────────────────────

#[tauri::command]
pub fn start_relay(config: RelayConfig, state: State<AppState>) -> Result<(), String> {
    let mut relay_guard = state.relay.lock().map_err(|_| "state lock poisoned")?;
    if matches!(*relay_guard, RelayState::Running { .. }) {
        return Err("relay is already running".into());
    }

    config.validate().map_err(|e| e.to_string())?;
    let toml = config::to_toml_string(&config);
    std::fs::write(&state.config_path, toml).map_err(|e| e.to_string())?;

    let config_path_str = state
        .config_path
        .to_string_lossy()
        .to_string();
    let relay = Relay::new(config, config_path_str);
    let source_stats = Arc::clone(&relay.source_stats);
    let dest_stats: Vec<Arc<Stats>> = relay.dest_stats.iter().map(Arc::clone).collect();
    let shutdown_tx = relay.shutdown_sender();
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = Arc::clone(&done);

    // spawn returns immediately; the lock is never held across an await point
    tauri::async_runtime::spawn(async move {
        if let Err(e) = relay.run().await {
            tracing::error!(error = %e, "relay terminated with error");
        }
        done_clone.store(true, Ordering::Release);
    });

    *relay_guard = RelayState::Running {
        shutdown_tx,
        source_stats,
        dest_stats,
        done,
    };
    Ok(())
}

// ── stop_relay ────────────────────────────────────────────────────────

#[tauri::command]
pub fn stop_relay(state: State<AppState>) -> Result<(), String> {
    let mut relay_guard = state.relay.lock().map_err(|_| "state lock poisoned")?;
    if let RelayState::Running { ref shutdown_tx, .. } = *relay_guard {
        let _ = shutdown_tx.send(true);
    }
    *relay_guard = RelayState::Stopped;
    Ok(())
}

// ── get_relay_status ──────────────────────────────────────────────────
// Also auto-transitions Running → Stopped when the relay task has exited.

#[tauri::command]
pub fn get_relay_status(state: State<AppState>) -> bool {
    let mut relay_guard = state
        .relay
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let exited = if let RelayState::Running { ref done, .. } = *relay_guard {
        done.load(Ordering::Acquire)
    } else {
        false
    };
    if exited {
        *relay_guard = RelayState::Stopped;
    }
    matches!(*relay_guard, RelayState::Running { .. })
}

// ── Stats payload ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatsPayload {
    pub source: StatsSnapshot,
    pub destinations: Vec<StatsSnapshot>,
}

// ── get_stats ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_stats(state: State<AppState>) -> Result<StatsPayload, String> {
    let relay_guard = state.relay.lock().map_err(|_| "state lock poisoned")?;
    match *relay_guard {
        RelayState::Running {
            ref source_stats,
            ref dest_stats,
            ..
        } => Ok(StatsPayload {
            source: source_stats.snapshot(),
            destinations: dest_stats.iter().map(|s| s.snapshot()).collect(),
        }),
        RelayState::Stopped => Err("relay is not running".into()),
    }
}

// ── get_local_ips ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_local_ips() -> Vec<String> {
    let mut ips = vec![
        "0.0.0.0".to_string(),
        "127.0.0.1".to_string(),
        "::".to_string(),
        "::1".to_string(),
    ];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            let ip = match iface.addr {
                if_addrs::IfAddr::V4(v4) => v4.ip.to_string(),
                if_addrs::IfAddr::V6(v6) => v6.ip.to_string(),
            };
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    ips
}

// ── get_broadcast_ips ────────────────────────────────────────────────

#[tauri::command]
pub fn get_broadcast_ips() -> Vec<String> {
    let mut ips = vec!["255.255.255.255".to_string()];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                if let Some(bcast) = v4.broadcast {
                    let s = bcast.to_string();
                    if !ips.contains(&s) {
                        ips.push(s);
                    }
                }
            }
        }
    }
    ips
}

// ── get_prefs / save_prefs ────────────────────────────────────────────

#[tauri::command]
pub fn get_prefs(state: State<AppState>) -> Result<Prefs, String> {
    let path = state
        .prefs_path
        .to_str()
        .ok_or("invalid prefs path")?;
    Prefs::load(path)
}

#[tauri::command]
pub fn save_prefs(prefs: Prefs, state: State<AppState>) -> Result<(), String> {
    let path = state
        .prefs_path
        .to_str()
        .ok_or("invalid prefs path")?;
    prefs.save(path)
}
