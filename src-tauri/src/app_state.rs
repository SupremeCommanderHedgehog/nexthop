// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

use crate::stats::Stats;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

pub enum RelayState {
    Stopped,
    Running {
        shutdown_tx: Arc<watch::Sender<bool>>,
        source_stats: Arc<Stats>,
        dest_stats: Vec<Arc<Stats>>,
        done: Arc<AtomicBool>,
    },
}

pub struct AppState {
    pub relay: Mutex<RelayState>,
    pub config_path: PathBuf,
    pub prefs_path: PathBuf,
}
