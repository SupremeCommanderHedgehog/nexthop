// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

use crate::relay::LiveStats;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub enum RelayState {
    Stopped,
    Running {
        shutdown_tx: Arc<watch::Sender<bool>>,
        /// Lock-free shared view: source stats first, then each destination
        /// in current-config order. The supervisor swaps the inner Vec on
        /// every add/remove, so every `get_stats` reflects the live set.
        live_stats: LiveStats,
        done: Arc<AtomicBool>,
    },
}

pub struct AppState {
    pub relay: Mutex<RelayState>,
    pub config_path: PathBuf,
    pub prefs_path: PathBuf,
}
