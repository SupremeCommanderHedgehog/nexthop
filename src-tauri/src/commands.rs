// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

use crate::app_state::{AppState, RelayState};
use crate::config::{
    self, CastMode, DestConfig, EndpointConfig, EndpointMode, GeneralConfig, OverflowPolicy,
    Protocol, RelayConfig,
};
use crate::prefs::Prefs;
use crate::relay::Relay;
use crate::stats::StatsSnapshot;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

// ── Default config returned when nexthop.toml does not exist yet ──

fn default_config() -> RelayConfig {
    RelayConfig {
        general: GeneralConfig {
            log_level: "info".into(),
            stats_interval_secs: 30,
            channel_capacity: 1024,
            max_payload_size: 65535,
            health_port: None,
            health_bind_addr: None,
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
            rate_limit: None,
            transforms: Vec::new(),
        }],
    }
}

// ── get_config ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_config(state: State<AppState>) -> Result<RelayConfig, String> {
    let path = state.config_path.to_str().ok_or("invalid config path")?;
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

/// Emitted to the frontend when the relay task exits. `run_id` is the
/// client-supplied id of the run that stopped, so the UI can ignore a late event
/// from a prior run; `error` is `None` for a graceful stop (user-initiated
/// shutdown) and `Some(msg)` when the relay terminated on its own with an error.
/// Lets the UI react to unexpected stops without polling.
#[derive(Clone, Serialize)]
struct RelayStoppedPayload {
    run_id: String,
    error: Option<String>,
}

/// Reaps a relay run's state and notifies the UI when its task ends — on the
/// normal path, an error return, or a panic inside `relay.run()`. Runs exactly
/// once when the spawned task's scope unwinds, so `Running` never gets stranded.
struct RelayExitGuard {
    app: AppHandle,
    run_id: String,
    error: Option<String>,
}

impl Drop for RelayExitGuard {
    fn drop(&mut self) {
        // Reap only if this run is still the current one — a newer run may have
        // already replaced it. The lock is released before the emit below.
        {
            let st = self.app.state::<AppState>();
            let mut guard = st.relay.lock().unwrap_or_else(|p| p.into_inner());
            if let RelayState::Running {
                run_id: ref cur, ..
            } = *guard
            {
                if *cur == self.run_id {
                    *guard = RelayState::Stopped;
                }
            }
        }
        let _ = self.app.emit(
            "relay-stopped",
            RelayStoppedPayload {
                run_id: self.run_id.clone(),
                error: self.error.take(),
            },
        );
    }
}

#[tauri::command]
pub fn start_relay(
    config: RelayConfig,
    run_id: String,
    state: State<AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let mut relay_guard = state.relay.lock().map_err(|_| "state lock poisoned")?;
    // The task reaps its own Running state on exit, so a live Running here means
    // a relay is genuinely still up.
    if matches!(*relay_guard, RelayState::Running { .. }) {
        return Err("relay is already running".into());
    }

    config.validate().map_err(|e| e.to_string())?;
    let toml = config::to_toml_string(&config);
    std::fs::write(&state.config_path, toml).map_err(|e| e.to_string())?;

    let config_path_str = state.config_path.to_string_lossy().to_string();
    let relay = Relay::new(config, config_path_str);
    let live_stats = Arc::clone(&relay.live_stats);
    let shutdown_tx = relay.shutdown_sender();
    let run_id_task = run_id.clone();

    // spawn returns immediately; the lock is never held across an await point
    tauri::async_runtime::spawn(async move {
        // The guard reaps state + emits `relay-stopped` when this scope exits,
        // whether relay.run() returns Ok/Err or panics.
        let mut exit = RelayExitGuard {
            app,
            run_id: run_id_task,
            error: None,
        };
        if let Err(e) = relay.run().await {
            tracing::error!(error = %e, "relay terminated with error");
            exit.error = Some(e.to_string());
        }
    });

    *relay_guard = RelayState::Running {
        run_id,
        shutdown_tx,
        live_stats,
    };
    Ok(())
}

// ── stop_relay ────────────────────────────────────────────────────────

#[tauri::command]
pub fn stop_relay(state: State<AppState>) -> Result<(), String> {
    let mut relay_guard = state.relay.lock().map_err(|_| "state lock poisoned")?;
    if let RelayState::Running {
        ref shutdown_tx, ..
    } = *relay_guard
    {
        let _ = shutdown_tx.send(true);
    }
    *relay_guard = RelayState::Stopped;
    Ok(())
}

// ── get_relay_status ──────────────────────────────────────────────────
// Returns the current run's id (for the frontend to correlate `relay-stopped`
// events) or `None` when stopped. The running task reaps its own state on exit,
// so no lazy transition is needed here.

#[tauri::command]
pub fn get_relay_status(state: State<AppState>) -> Option<String> {
    let relay_guard = state.relay.lock().unwrap_or_else(|p| p.into_inner());
    if let RelayState::Running { ref run_id, .. } = *relay_guard {
        Some(run_id.clone())
    } else {
        None
    }
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
        RelayState::Running { ref live_stats, .. } => {
            let snapshot = live_stats.load_full();
            let Some(source) = snapshot.first() else {
                return Err("relay is starting up".into());
            };
            Ok(StatsPayload {
                source: source.snapshot(),
                destinations: snapshot[1..].iter().map(|s| s.snapshot()).collect(),
            })
        }
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
    let path = state.prefs_path.to_str().ok_or("invalid prefs path")?;
    Prefs::load(path)
}

#[tauri::command]
pub fn save_prefs(prefs: Prefs, state: State<AppState>) -> Result<(), String> {
    let path = state.prefs_path.to_str().ok_or("invalid prefs path")?;
    prefs.save(path)
}
