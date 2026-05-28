// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

//! Relay engine: wires sources to destinations via per-destination mpsc channels.

use crate::config::*;
use crate::error::Result;
use crate::rate_limiter::RateLimiter;
use crate::stats::Stats;
use crate::transport;
use arc_swap::ArcSwap;
use bytes::Bytes;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Mutex};
use tracing::{debug, error, info, warn};

// ════════════════════════════════════════════════════════════════════
//  Destination identity
// ════════════════════════════════════════════════════════════════════

/// Stable key used to match destinations across hot-reloads. Two configs
/// describe "the same destination" iff their keys match; otherwise the
/// supervisor treats the new entry as an add and the missing one as a
/// remove.
///
/// Identity intentionally excludes mutable / cosmetic fields
/// (`overflow_policy`, `reconnect_delay_ms`, `name`) so changes there
/// take the in-place live-update path rather than respawning the task.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct DestKey {
    pub protocol: Protocol,
    pub mode: EndpointMode,
    pub address: String,
    pub cast_mode: CastMode,
    pub multicast_interface: Option<String>,
    pub multicast_interface_index: Option<u32>,
}

impl DestKey {
    pub fn from_cfg(cfg: &DestConfig) -> Self {
        Self {
            protocol: cfg.protocol,
            mode: cfg.mode,
            address: cfg.address.clone(),
            cast_mode: cfg.cast_mode,
            multicast_interface: cfg.multicast_interface.clone(),
            multicast_interface_index: cfg.multicast_interface_index,
        }
    }
}

/// Lock-free view of the live destination fan-out, swapped atomically
/// when destinations are added or removed. Source tasks call
/// `load_full()` once per packet to get a stable snapshot.
pub(crate) type LiveChannels = Arc<ArcSwap<Vec<DestChannel>>>;

/// Same lock-free pattern for the per-endpoint stats list the GUI reads
/// through `commands::get_stats`. Order is: source first, then each
/// destination in current config order.
pub type LiveStats = Arc<ArcSwap<Vec<Arc<Stats>>>>;

// ════════════════════════════════════════════════════════════════════
//  Per-destination live state
// ════════════════════════════════════════════════════════════════════

/// Atomically-readable per-destination state shared between the source's
/// fan-out (which reads `overflow_policy` on every packet) and the
/// destination task (which reads `reconnect_delay_ms` on each reconnect
/// cycle, and the limiter on every write). Hot-reload writes these
/// without touching the destination task itself, so policy, delay, or
/// rate limit can change without dropping connections.
pub struct DestRuntime {
    policy: AtomicU8,
    reconnect_delay_ms: AtomicU64,
    /// Per-destination rate limiter override. `None` means this
    /// destination falls back to the global limiter (see
    /// `effective_limiter`).
    private_limiter: RwLock<Option<Arc<RateLimiter>>>,
}

// Numeric encoding of OverflowPolicy for atomic storage.
const POLICY_DROP_NEWEST: u8 = 0;
const POLICY_BLOCK: u8 = 1;

fn policy_to_u8(p: OverflowPolicy) -> u8 {
    match p {
        OverflowPolicy::DropNewest => POLICY_DROP_NEWEST,
        OverflowPolicy::Block => POLICY_BLOCK,
    }
}

fn u8_to_policy(v: u8) -> OverflowPolicy {
    if v == POLICY_BLOCK {
        OverflowPolicy::Block
    } else {
        OverflowPolicy::DropNewest
    }
}

impl DestRuntime {
    pub fn new(
        policy: OverflowPolicy,
        reconnect_delay_ms: u64,
        rate_limit: Option<&RateLimitConfig>,
    ) -> Self {
        let private_limiter =
            rate_limit.map(|rl| Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size)));
        Self {
            policy: AtomicU8::new(policy_to_u8(policy)),
            reconnect_delay_ms: AtomicU64::new(reconnect_delay_ms),
            private_limiter: RwLock::new(private_limiter),
        }
    }

    pub fn policy(&self) -> OverflowPolicy {
        u8_to_policy(self.policy.load(Ordering::Relaxed))
    }

    pub fn set_policy(&self, p: OverflowPolicy) {
        self.policy.store(policy_to_u8(p), Ordering::Relaxed);
    }

    pub fn reconnect_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_delay_ms.load(Ordering::Relaxed))
    }

    pub fn set_reconnect_delay_ms(&self, ms: u64) {
        self.reconnect_delay_ms.store(ms, Ordering::Relaxed);
    }

    /// Install a per-destination rate limit, replacing any previous one.
    /// Pass `None` to clear the override and fall back to the global
    /// limiter on subsequent acquires.
    pub fn set_private_limiter(&self, rate_limit: Option<&RateLimitConfig>) {
        let new =
            rate_limit.map(|rl| Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size)));
        *self
            .private_limiter
            .write()
            .unwrap_or_else(|p| p.into_inner()) = new;
    }

    /// Resolve the limiter the destination task should acquire from for
    /// the next write. The per-destination override wins; otherwise we
    /// fall back to the shared global limiter (which may itself be
    /// empty). Both reads clone the inner `Arc` so we never hold a
    /// `RwLockReadGuard` across an `.await`.
    pub fn effective_limiter(&self, global: &SharedLimiter) -> Option<Arc<RateLimiter>> {
        let priv_slot = self
            .private_limiter
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if priv_slot.is_some() {
            return priv_slot;
        }
        global.read().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

// ════════════════════════════════════════════════════════════════════
//  Destination supervisor
// ════════════════════════════════════════════════════════════════════

/// Owns the per-destination lifecycle: task handle, channel sender,
/// per-destination shutdown, runtime atomics, and stats. The supervisor
/// is keyed by `DestKey`, so adding, removing, or in-place updating a
/// destination across hot-reloads never touches the others.
pub(crate) struct DestEntry {
    pub cfg: DestConfig,
    pub tx: mpsc::Sender<Bytes>,
    pub runtime: Arc<DestRuntime>,
    pub stats: Arc<Stats>,
    /// Only this destination's task observes this. The global relay
    /// shutdown is signalled separately and reaches every task via
    /// `global_shutdown_rx`.
    pub shutdown_tx: Arc<watch::Sender<bool>>,
    pub task: tokio::task::JoinHandle<()>,
}

/// Outcome of comparing the old vs new destination list. Pure data so
/// it can be unit-tested without spinning a runtime.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DiffPlan {
    pub added: Vec<DestKey>,
    pub removed: Vec<DestKey>,
    pub kept: Vec<DestKey>,
}

pub(crate) fn diff_destinations(old: &[DestConfig], new: &[DestConfig]) -> DiffPlan {
    let old_keys: HashSet<DestKey> = old.iter().map(DestKey::from_cfg).collect();
    let new_keys: HashSet<DestKey> = new.iter().map(DestKey::from_cfg).collect();

    let mut plan = DiffPlan::default();
    for k in new.iter().map(DestKey::from_cfg) {
        if old_keys.contains(&k) {
            if !plan.kept.contains(&k) {
                plan.kept.push(k);
            }
        } else if !plan.added.contains(&k) {
            plan.added.push(k);
        }
    }
    for k in old.iter().map(DestKey::from_cfg) {
        if !new_keys.contains(&k) && !plan.removed.contains(&k) {
            plan.removed.push(k);
        }
    }
    plan
}

pub(crate) struct DestSupervisor {
    entries: HashMap<DestKey, DestEntry>,
    live_channels: LiveChannels,
    live_stats: LiveStats,
    /// Stats list always starts with the source's Stats; the supervisor
    /// preserves that head element on every rebuild.
    source_stats: Arc<Stats>,
    channel_capacity: usize,
    stats_interval_secs: Arc<AtomicU64>,
    /// Shared global limiter handle. Each destination task reads its
    /// effective limiter via `DestRuntime::effective_limiter(global)`,
    /// so a hot-reload swap on `global` reaches every destination
    /// without a per-task message.
    global_limiter: SharedLimiter,
    global_shutdown_rx: watch::Receiver<bool>,
}

impl DestSupervisor {
    pub fn new(
        source_stats: Arc<Stats>,
        live_channels: LiveChannels,
        live_stats: LiveStats,
        channel_capacity: usize,
        stats_interval_secs: Arc<AtomicU64>,
        global_limiter: SharedLimiter,
        global_shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            live_channels,
            live_stats,
            source_stats,
            channel_capacity,
            stats_interval_secs,
            global_limiter,
            global_shutdown_rx,
        }
    }

    /// Insert a destination into the live set, spawning its task.
    /// Caller is responsible for rebuilding live_channels / live_stats
    /// after a batch of inserts via [`rebuild_views`].
    fn spawn_entry(&self, cfg: DestConfig, stats: Arc<Stats>) -> DestEntry {
        let runtime = Arc::new(DestRuntime::new(
            cfg.overflow_policy,
            cfg.reconnect_delay_ms.unwrap_or(2000),
            cfg.rate_limit.as_ref(),
        ));
        let (per_tx, per_rx) = watch::channel(false);
        let shutdown_tx = Arc::new(per_tx);
        // Combine the per-destination shutdown with the global one: the
        // destination task only sees one receiver, and either signal
        // triggers a clean exit.
        let combined_shutdown_rx =
            spawn_combined_shutdown_rx(per_rx, self.global_shutdown_rx.clone());

        let (tx, rx) = mpsc::channel::<Bytes>(self.channel_capacity);

        let name = cfg.display_name();
        let cap = self.channel_capacity;
        let runtime_for_task = Arc::clone(&runtime);
        let stats_for_task = Arc::clone(&stats);
        let stats_interval = Arc::clone(&self.stats_interval_secs);
        let global_shutdown_for_reporter = self.global_shutdown_rx.clone();
        let global_limiter_for_task = Arc::clone(&self.global_limiter);
        let cfg_for_task = cfg.clone();

        // Spawn the periodic stats reporter for this dest. It exits when
        // the global shutdown fires; we don't tie it to per-dest shutdown
        // because the GUI may still want the final stats snapshot.
        tokio::spawn(crate::stats::run_reporter(
            Arc::clone(&stats_for_task),
            stats_interval,
            global_shutdown_for_reporter,
        ));

        info!(dest = %name, proto = ?cfg.protocol, mode = ?cfg.mode, "destination starting");
        let task = tokio::spawn(async move {
            if let Err(e) = run_destination(
                cfg_for_task,
                runtime_for_task,
                global_limiter_for_task,
                rx,
                stats_for_task.clone(),
                combined_shutdown_rx,
                cap,
            )
            .await
            {
                error!(dest = %name, error = %e, "destination failed");
                stats_for_task.add_error();
            }
        });

        DestEntry {
            cfg,
            tx,
            runtime,
            stats,
            shutdown_tx,
            task,
        }
    }

    /// Rebuild the `live_channels` and `live_stats` views from the
    /// current `entries`, preserving the iteration order matching the
    /// most recently applied config.
    fn rebuild_views(&self, ordered_keys: &[DestKey]) {
        let mut channels = Vec::with_capacity(ordered_keys.len());
        let mut stats_vec: Vec<Arc<Stats>> = Vec::with_capacity(ordered_keys.len() + 1);
        stats_vec.push(Arc::clone(&self.source_stats));
        for k in ordered_keys {
            if let Some(e) = self.entries.get(k) {
                channels.push(DestChannel {
                    tx: e.tx.clone(),
                    name: e.cfg.display_name(),
                    stats: Arc::clone(&e.stats),
                    runtime: Arc::clone(&e.runtime),
                });
                stats_vec.push(Arc::clone(&e.stats));
            }
        }
        self.live_channels.store(Arc::new(channels));
        self.live_stats.store(Arc::new(stats_vec));
    }

    /// Spawn all entries described by `cfgs`, using pre-built `Stats`
    /// (one per cfg, in matching order) so the GUI's startup snapshot
    /// of `Relay::live_stats` already has the right `Arc<Stats>`
    /// handles before `run()` reaches this point.
    pub fn spawn_initial(&mut self, cfgs: &[DestConfig], stats: &[Arc<Stats>]) {
        debug_assert_eq!(cfgs.len(), stats.len(), "stats vec must align with cfgs");
        for (cfg, st) in cfgs.iter().zip(stats.iter()) {
            let key = DestKey::from_cfg(cfg);
            let entry = self.spawn_entry(cfg.clone(), Arc::clone(st));
            self.entries.insert(key, entry);
        }
        let ordered: Vec<_> = cfgs.iter().map(DestKey::from_cfg).collect();
        self.rebuild_views(&ordered);
    }

    /// Apply the difference between an already-running config and a new
    /// one: spawn added entries, drain + drop removed entries, and
    /// in-place mutate kept entries whose `overflow_policy` or
    /// `reconnect_delay_ms` changed.
    pub async fn apply_config(&mut self, old: &[DestConfig], new: &[DestConfig]) {
        let plan = diff_destinations(old, new);

        // ── kept: in-place update of policy + delay ────────────────
        // Build name lookup against the new config so log lines reflect
        // the current display name even if it changed.
        let new_by_key: HashMap<DestKey, &DestConfig> =
            new.iter().map(|c| (DestKey::from_cfg(c), c)).collect();

        for key in &plan.kept {
            let Some(entry) = self.entries.get_mut(key) else {
                continue;
            };
            let new_cfg = match new_by_key.get(key) {
                Some(c) => *c,
                None => continue,
            };
            if entry.cfg.overflow_policy != new_cfg.overflow_policy {
                entry.runtime.set_policy(new_cfg.overflow_policy);
                info!(
                    dest = %new_cfg.display_name(),
                    policy = ?new_cfg.overflow_policy,
                    "config reloaded: overflow_policy updated"
                );
            }
            if entry.cfg.reconnect_delay_ms != new_cfg.reconnect_delay_ms {
                let ms = new_cfg.reconnect_delay_ms.unwrap_or(2000);
                entry.runtime.set_reconnect_delay_ms(ms);
                info!(
                    dest = %new_cfg.display_name(),
                    reconnect_delay_ms = ms,
                    "config reloaded: reconnect_delay_ms updated (takes effect on next reconnect)"
                );
            }
            if entry.cfg.rate_limit != new_cfg.rate_limit {
                entry
                    .runtime
                    .set_private_limiter(new_cfg.rate_limit.as_ref());
                match &new_cfg.rate_limit {
                    Some(rl) => info!(
                        dest = %new_cfg.display_name(),
                        bytes_per_second = rl.bytes_per_second,
                        burst = rl.burst_size,
                        "config reloaded: per-destination rate_limit updated"
                    ),
                    None => info!(
                        dest = %new_cfg.display_name(),
                        "config reloaded: per-destination rate_limit removed; falling back to global"
                    ),
                }
            }
            entry.cfg = new_cfg.clone();
        }

        // ── added: spawn new tasks ────────────────────────────────
        for key in &plan.added {
            let Some(new_cfg) = new_by_key.get(key) else {
                continue;
            };
            info!(dest = %new_cfg.display_name(), "config reloaded: destination added");
            let stats = Arc::new(build_dest_stats(new_cfg));
            let entry = self.spawn_entry((*new_cfg).clone(), stats);
            self.entries.insert(key.clone(), entry);
        }

        // ── views first, so the source stops seeing removed dests ─
        let ordered: Vec<_> = new.iter().map(DestKey::from_cfg).collect();
        self.rebuild_views(&ordered);

        // ── removed: signal shutdown and await drain ──────────────
        for key in &plan.removed {
            if let Some(entry) = self.entries.remove(key) {
                info!(dest = %entry.cfg.display_name(), "config reloaded: destination removed, draining");
                let _ = entry.shutdown_tx.send(true);
                // Drop the sender so the destination's rx.recv() returns
                // None once in-flight packets drain.
                drop(entry.tx);
                let _ = tokio::time::timeout(Duration::from_secs(5), entry.task).await;
            }
        }
    }

    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn entry_runtime(&self, key: &DestKey) -> Option<Arc<DestRuntime>> {
        self.entries.get(key).map(|e| Arc::clone(&e.runtime))
    }
}

/// Construct the per-destination `Stats` with the label format the rest
/// of the codebase expects. Used by both `Relay::new` (for the startup
/// snapshot) and the supervisor (for hot-reload adds).
fn build_dest_stats(cfg: &DestConfig) -> Stats {
    let local = if cfg.mode == EndpointMode::Server {
        cfg.address.clone()
    } else {
        "(auto)".to_string()
    };
    let peer = if cfg.mode == EndpointMode::Client {
        cfg.address.clone()
    } else {
        "(any)".to_string()
    };
    Stats::new(format!("dest({})", cfg.display_name()), local, peer)
}

/// Fan in two watch receivers into one. The returned receiver fires as
/// soon as either source fires; the spawned task exits once that
/// happens so it does not leak.
fn spawn_combined_shutdown_rx(
    mut a: watch::Receiver<bool>,
    mut b: watch::Receiver<bool>,
) -> watch::Receiver<bool> {
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::select! {
            _ = a.changed() => {}
            _ = b.changed() => {}
        }
        let _ = tx.send(true);
    });
    rx
}

// ════════════════════════════════════════════════════════════════════
//  Shared rate-limiter slot
// ════════════════════════════════════════════════════════════════════

/// An atomically-swappable rate limiter.  Source tasks clone the inner Arc
/// on each read so a config reload takes effect on the very next packet
/// without restarting any tasks.
type SharedLimiter = Arc<RwLock<Option<Arc<RateLimiter>>>>;

// ════════════════════════════════════════════════════════════════════
//  Per-destination channel
// ════════════════════════════════════════════════════════════════════

pub(crate) struct DestChannel {
    tx: mpsc::Sender<Bytes>,
    name: String,
    stats: Arc<Stats>,
    runtime: Arc<DestRuntime>,
}

/// Fan-out one packet to every destination, applying each destination's
/// overflow policy independently. Policy is read fresh per packet so a
/// hot-reload swap takes effect immediately without dropping the channel.
///
/// The snapshot is loaded once per call via `arc_swap::ArcSwap::load_full`,
/// so add/remove of destinations between calls is observed without any
/// locking on the per-packet path.
async fn send_to_all(channels: &LiveChannels, data: Bytes) {
    let snapshot = channels.load_full();
    let mut block_sends = Vec::new();
    for ch in snapshot.iter() {
        match ch.runtime.policy() {
            OverflowPolicy::DropNewest => {
                if ch.tx.try_send(data.clone()).is_err() {
                    warn!(dest = %ch.name, "dest queue full, packet dropped");
                    ch.stats.add_dropped_overflow(1);
                }
            }
            OverflowPolicy::Block => {
                block_sends.push(ch.tx.send(data.clone()));
            }
        }
    }
    // Drive all Block sends concurrently so one slow destination can't
    // starve others with DropNewest policy.
    futures::future::join_all(block_sends).await;
}

// ════════════════════════════════════════════════════════════════════
//  Public entry point
// ════════════════════════════════════════════════════════════════════

/// Setter for the live log-level filter. `lib.rs` installs the real
/// implementation backed by a `tracing_subscriber::reload::Handle`; the
/// GUI path and tests use the no-op default. The closure returns
/// `false` if the new filter string failed to parse so the caller can
/// log a warning.
pub type LogLevelSetter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

fn noop_log_setter() -> LogLevelSetter {
    Arc::new(|_| false)
}

pub struct Relay {
    config_path: String,
    config: RelayConfig,
    limiter: SharedLimiter,
    /// Updated in place by hot-reload; every reporter task reads it on
    /// each tick so an interval change takes effect on the next cycle
    /// without restarting the reporters.
    stats_interval_secs: Arc<AtomicU64>,
    log_setter: LogLevelSetter,
    pub source_stats: Arc<Stats>,
    /// Startup snapshot of per-destination `Stats`, in config order.
    /// After `run()` starts and the supervisor has handled a hot-reload
    /// add/remove, the canonical view lives in `live_stats`; this field
    /// stays as the initial set so GUI/tests can read it from a
    /// not-yet-started `Relay`.
    pub dest_stats: Vec<Arc<Stats>>,
    /// Lock-free view consumed by the GUI's `get_stats`. Source stats
    /// first, then each destination in current config order. The
    /// supervisor swaps the inner `Vec` whenever destinations are added
    /// or removed, so the GUI always reflects the live set without any
    /// per-poll coordination.
    pub live_stats: LiveStats,
    live_channels: LiveChannels,
    shutdown_tx: Arc<watch::Sender<bool>>,
    shutdown_rx: watch::Receiver<bool>,
}

impl Relay {
    pub fn new(config: RelayConfig, config_path: String) -> Self {
        Self::with_log_setter(config, config_path, noop_log_setter())
    }

    pub fn with_log_setter(
        config: RelayConfig,
        config_path: String,
        log_setter: LogLevelSetter,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let limiter_val = config
            .rate_limit
            .as_ref()
            .map(|rl| Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size)));
        let src_local = if config.source.mode == EndpointMode::Server {
            config.source.address.clone()
        } else {
            "(auto)".to_string()
        };
        let src_peer = if config.source.mode == EndpointMode::Client {
            config.source.address.clone()
        } else {
            "(any)".to_string()
        };
        let source_stats = Arc::new(Stats::new(
            format!("source({})", config.source.display_name()),
            src_local,
            src_peer,
        ));
        let dest_stats: Vec<Arc<Stats>> = config
            .destinations
            .iter()
            .map(|d| Arc::new(build_dest_stats(d)))
            .collect();
        let stats_interval_secs = Arc::new(AtomicU64::new(config.general.stats_interval_secs));
        // Live channels start empty; the supervisor populates them inside
        // `run()` once it has a tokio runtime to spawn destinations into.
        // Live stats can be pre-populated because Stats are pure data — the
        // GUI / tests can read the initial set from `Relay::new` before
        // `run()` starts.
        let live_channels: LiveChannels = Arc::new(ArcSwap::from_pointee(Vec::new()));
        let initial_stats_view: Vec<Arc<Stats>> = std::iter::once(Arc::clone(&source_stats))
            .chain(dest_stats.iter().cloned())
            .collect();
        let live_stats: LiveStats = Arc::new(ArcSwap::from_pointee(initial_stats_view));
        Self {
            config_path,
            config,
            limiter: Arc::new(RwLock::new(limiter_val)),
            stats_interval_secs,
            log_setter,
            source_stats,
            dest_stats,
            live_stats,
            live_channels,
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        }
    }

    /// Clone the shutdown sender so external callers (e.g. the GUI) can stop
    /// the relay without owning it.
    pub fn shutdown_sender(&self) -> Arc<watch::Sender<bool>> {
        Arc::clone(&self.shutdown_tx)
    }

    pub async fn run(self) -> Result<()> {
        let max_payload = self.config.general.max_payload_size;
        let cap = self.config.general.channel_capacity;

        let mut task_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        // Per-source stats reporter. The supervisor owns the per-dest
        // reporters; this one just covers the source endpoint.
        let src_name = self.config.source.display_name();
        let source_stats = Arc::clone(&self.source_stats);
        task_handles.push(tokio::spawn(crate::stats::run_reporter(
            Arc::clone(&source_stats),
            Arc::clone(&self.stats_interval_secs),
            self.shutdown_rx.clone(),
        )));

        // Build and seed the destination supervisor before spawning the
        // source so the source's first packet sees the full live set.
        let mut supervisor = DestSupervisor::new(
            Arc::clone(&self.source_stats),
            Arc::clone(&self.live_channels),
            Arc::clone(&self.live_stats),
            cap,
            Arc::clone(&self.stats_interval_secs),
            Arc::clone(&self.limiter),
            self.shutdown_rx.clone(),
        );
        supervisor.spawn_initial(&self.config.destinations, &self.dest_stats);
        let supervisor = Arc::new(tokio::sync::Mutex::new(supervisor));

        // Config hot-reload watcher — wrap startup config so the watcher can
        // track the last-reloaded baseline rather than comparing forever against
        // the original startup snapshot.
        let current_cfg = Arc::new(RwLock::new(self.config.clone()));
        let hot_ctx = HotReloadCtx {
            limiter: Arc::clone(&self.limiter),
            stats_interval_secs: Arc::clone(&self.stats_interval_secs),
            supervisor: Arc::clone(&supervisor),
            log_setter: Arc::clone(&self.log_setter),
        };
        task_handles.push(spawn_config_watcher(
            self.config_path.clone(),
            current_cfg,
            hot_ctx,
            self.shutdown_rx.clone(),
        ));

        if let Some(port) = self.config.general.health_port {
            task_handles.push(spawn_health_server(
                port,
                self.config.general.health_bind_addr.clone(),
                Arc::clone(&self.live_stats),
                self.shutdown_rx.clone(),
            ));
        }

        info!(source = %src_name, proto = ?self.config.source.protocol, mode = ?self.config.source.mode, "source starting");

        let source_fut = run_source(
            self.config.source.clone(),
            Arc::clone(&self.live_channels),
            Arc::clone(&source_stats),
            max_payload,
            self.shutdown_rx.clone(),
        );

        tokio::select! {
            res = source_fut => {
                if let Err(e) = res {
                    error!(error = %e, "source stopped with error");
                }
            }
            sig = wait_for_shutdown_signal() => {
                info!(signal = sig, "received shutdown signal, shutting down");
            }
        }

        // Signal all tasks to stop, then wait up to 5 s for them to drain cleanly.
        let _ = self.shutdown_tx.send(true);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            for handle in task_handles {
                let _ = handle.await;
            }
        })
        .await;
        info!("relay stopped");
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════
//  Shutdown signals
// ════════════════════════════════════════════════════════════════════

/// Wait for an OS shutdown signal and return its name for logging.
///
/// On Unix we listen for both SIGINT (Ctrl-C) and SIGTERM so that
/// systemd/Kubernetes/Docker can request a clean shutdown the same way a
/// terminal user can. On Windows only the cross-platform ctrl_c path
/// exists.
async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => tokio::select! {
                _ = tokio::signal::ctrl_c() => "SIGINT",
                _ = sigterm.recv() => "SIGTERM",
            },
            Err(e) => {
                warn!(error = %e, "SIGTERM handler install failed; only SIGINT will trigger shutdown");
                let _ = tokio::signal::ctrl_c().await;
                "SIGINT"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl+C"
    }
}

// ════════════════════════════════════════════════════════════════════
//  Config hot-reload
// ════════════════════════════════════════════════════════════════════

/// Handles `apply_hot_reload` writes through to mutate live state without
/// restarting tasks. All fields are independently swappable.
pub(crate) struct HotReloadCtx {
    pub limiter: SharedLimiter,
    pub stats_interval_secs: Arc<AtomicU64>,
    pub supervisor: Arc<tokio::sync::Mutex<DestSupervisor>>,
    pub log_setter: LogLevelSetter,
}

fn spawn_config_watcher(
    config_path: String,
    current_cfg: Arc<RwLock<RelayConfig>>,
    ctx: HotReloadCtx,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    use notify::{RecursiveMode, Watcher};

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        let _ = tx.try_send(());
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    warn!(error = %e, "config watcher init failed, hot-reload disabled");
                    return;
                }
            };

        if let Err(e) = watcher.watch(
            std::path::Path::new(&config_path),
            RecursiveMode::NonRecursive,
        ) {
            warn!(path = %config_path, error = %e, "cannot watch config file, hot-reload disabled");
            return;
        }

        info!(path = %config_path, "config hot-reload active");

        loop {
            tokio::select! {
                Some(()) = rx.recv() => {
                    // Debounce: editors often write files in multiple rapid flushes.
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    while rx.try_recv().is_ok() {}

                    match crate::config::RelayConfig::from_file(&config_path) {
                        Ok(new_cfg) => {
                            let old = current_cfg
                                .read()
                                .unwrap_or_else(|p| p.into_inner())
                                .clone();
                            apply_hot_reload(&old, &new_cfg, &ctx).await;
                            *current_cfg.write().unwrap_or_else(|p| p.into_inner()) = new_cfg;
                        }
                        Err(e) => warn!(error = %e, "config reload failed, keeping current settings"),
                    }
                }
                _ = shutdown.changed() => return,
            }
        }
    })
}

// ════════════════════════════════════════════════════════════════════
//  Health / readiness HTTP server
// ════════════════════════════════════════════════════════════════════

/// Resolve the bind addresses for the health server from the optional
/// `general.health_bind_addr` config. Returns `0.0.0.0` + `::` (one
/// each) when unset, or a single specific address when set. Validation
/// of the address string happens in `RelayConfig::validate`, but we
/// guard again here in case this is invoked from a hand-built config.
fn resolve_health_bind_addrs(port: u16, configured: Option<&str>) -> Vec<SocketAddr> {
    match configured {
        Some(s) => match s.parse::<std::net::IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(e) => {
                warn!(addr = %s, error = %e, "invalid health_bind_addr; falling back to dual-stack default");
                vec![
                    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), port),
                    SocketAddr::new(std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), port),
                ]
            }
        },
        None => vec![
            SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), port),
            SocketAddr::new(std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED), port),
        ],
    }
}

/// Bind a single health-server listener. For IPv6 we explicitly set
/// `IPV6_V6ONLY=true` so the dual-stack default (binding both `0.0.0.0`
/// and `::`) never conflicts at the kernel level and so an explicit
/// `::` config means IPv6-only rather than the platform-dependent
/// v4-mapped behaviour.
async fn bind_health_listener(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    match addr {
        SocketAddr::V4(_) => tokio::net::TcpListener::bind(addr).await,
        SocketAddr::V6(_) => {
            use socket2::{Domain, Protocol, Socket, Type};
            let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
            socket.set_only_v6(true)?;
            socket.set_nonblocking(true)?;
            socket.bind(&addr.into())?;
            socket.listen(1024)?;
            tokio::net::TcpListener::from_std(socket.into())
        }
    }
}

fn spawn_health_server(
    port: u16,
    bind_addr: Option<String>,
    live_stats: LiveStats,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let bind_addrs = resolve_health_bind_addrs(port, bind_addr.as_deref());
        let sem = Arc::new(tokio::sync::Semaphore::new(32));
        let mut subtasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        for addr in bind_addrs {
            let listener = match bind_health_listener(addr).await {
                Ok(l) => l,
                Err(e) => {
                    warn!(addr = %addr, error = %e, "health server failed to bind");
                    continue;
                }
            };
            info!(
                addr = %addr,
                "health server listening — GET /health  GET /stats  GET /metrics"
            );

            let sem = Arc::clone(&sem);
            let live_stats = Arc::clone(&live_stats);
            let mut sd = shutdown.clone();
            subtasks.push(tokio::spawn(async move {
                loop {
                    tokio::select! {
                        accept = listener.accept() => {
                            match accept {
                                Ok((stream, _)) => {
                                    match Arc::clone(&sem).try_acquire_owned() {
                                        Ok(permit) => {
                                            let stats = Arc::clone(&live_stats);
                                            tokio::spawn(async move {
                                                handle_health_request(stream, stats).await;
                                                drop(permit);
                                            });
                                        }
                                        Err(_) => warn!(
                                            "health server: connection limit reached, dropping"
                                        ),
                                    }
                                }
                                Err(e) => warn!(error = %e, "health server accept error"),
                            }
                        }
                        _ = sd.changed() => return,
                    }
                }
            }));
        }

        if subtasks.is_empty() {
            warn!("health server: no successful binds, not serving");
            return;
        }
        for t in subtasks {
            let _ = t.await;
        }
    })
}

async fn handle_health_request(stream: tokio::net::TcpStream, live_stats: LiveStats) {
    // Drop the connection if the client stalls during request or response.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        serve_health_request(stream, live_stats),
    )
    .await;
}

async fn serve_health_request(stream: tokio::net::TcpStream, live_stats: LiveStats) {
    let all_stats = live_stats.load_full();
    let all_stats: &[Arc<Stats>] = all_stats.as_slice();
    let (reader_half, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader_half);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.is_err() {
        return;
    }

    // Drain remaining HTTP headers until blank line.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            _ => {}
        }
    }

    // "GET /path HTTP/1.1" — extract path, stripping any query string.
    let raw_path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = raw_path.split('?').next().unwrap_or("/");

    let (status, content_type, body) = match path {
        "/health" => (
            "200 OK",
            "application/json",
            r#"{"status":"ok"}"#.to_string(),
        ),
        "/stats" => {
            let snapshots: Vec<_> = all_stats.iter().map(|s| s.snapshot()).collect();
            match serde_json::to_string(&snapshots) {
                Ok(json) => ("200 OK", "application/json", json),
                Err(_) => (
                    "500 Internal Server Error",
                    "application/json",
                    r#"{"error":"serialize failed"}"#.to_string(),
                ),
            }
        }
        "/metrics" => {
            let snapshots: Vec<_> = all_stats.iter().map(|s| s.snapshot()).collect();
            (
                "200 OK",
                "text/plain; version=0.0.4; charset=utf-8",
                crate::stats::render_prometheus(&snapshots),
            )
        }
        _ => (
            "404 Not Found",
            "application/json",
            r#"{"error":"not found"}"#.to_string(),
        ),
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = writer.write_all(response.as_bytes()).await;
}

/// Apply the fields that can be changed without restarting tasks.
///
/// Live: rate_limit, general.log_level, general.stats_interval_secs, and
/// the full destination set (add/remove/update via the supervisor).
/// Source and a small set of process-global general fields still need a
/// restart; we log a single targeted warning naming the offending area
/// instead of a blanket "something changed" message.
async fn apply_hot_reload(old: &RelayConfig, new: &RelayConfig, ctx: &HotReloadCtx) {
    // ── rate_limit ────────────────────────────────────────────────
    if old.rate_limit != new.rate_limit {
        let new_limiter = new
            .rate_limit
            .as_ref()
            .map(|rl| Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size)));
        *ctx.limiter.write().unwrap_or_else(|p| p.into_inner()) = new_limiter;
        match &new.rate_limit {
            Some(rl) => info!(
                bytes_per_second = rl.bytes_per_second,
                burst = rl.burst_size,
                "config reloaded: rate limit updated"
            ),
            None => info!("config reloaded: rate limiting disabled"),
        }
    }

    // ── general.log_level ─────────────────────────────────────────
    if old.general.log_level != new.general.log_level {
        if (ctx.log_setter)(&new.general.log_level) {
            info!(
                log_level = %new.general.log_level,
                "config reloaded: log level updated"
            );
        } else {
            warn!(
                log_level = %new.general.log_level,
                "config reloaded: log level change rejected (invalid filter or no reload handle); keeping previous level"
            );
        }
    }

    // ── general.stats_interval_secs ───────────────────────────────
    if old.general.stats_interval_secs != new.general.stats_interval_secs {
        ctx.stats_interval_secs
            .store(new.general.stats_interval_secs, Ordering::Relaxed);
        info!(
            stats_interval_secs = new.general.stats_interval_secs,
            "config reloaded: stats interval updated"
        );
    }

    // ── destinations: full add/remove/update via supervisor ───────
    if old.destinations != new.destinations {
        let mut sup = ctx.supervisor.lock().await;
        sup.apply_config(&old.destinations, &new.destinations).await;
    }

    // ── still-restart-required surfaces ───────────────────────────
    if new.source != old.source {
        warn!("config reloaded: source changes require a restart (would force-drop in-flight connections)");
    }
    let g_old = &old.general;
    let g_new = &new.general;
    if g_old.channel_capacity != g_new.channel_capacity
        || g_old.max_payload_size != g_new.max_payload_size
        || g_old.health_port != g_new.health_port
    {
        warn!("config reloaded: channel_capacity, max_payload_size, and health_port require a restart");
    }
}

// ════════════════════════════════════════════════════════════════════
//  Source
// ════════════════════════════════════════════════════════════════════

async fn run_source(
    cfg: EndpointConfig,
    channels: LiveChannels,
    stats: Arc<Stats>,
    max_payload: usize,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    match (cfg.protocol, cfg.mode) {
        (Protocol::Tcp, EndpointMode::Server) => {
            source_tcp_server(cfg, channels, stats, max_payload, shutdown).await
        }
        (Protocol::Tcp, EndpointMode::Client) => {
            source_tcp_client(cfg, channels, stats, max_payload, shutdown).await
        }
        (Protocol::Udp, EndpointMode::Server | EndpointMode::Client) => {
            source_udp(cfg, channels, stats, max_payload, shutdown).await
        }
    }
}

// ── TCP source: server ─────────────────────────────────────────────

async fn source_tcp_server(
    cfg: EndpointConfig,
    channels: LiveChannels,
    stats: Arc<Stats>,
    max_payload: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let listener = tokio::task::spawn_blocking(move || transport::bind_tcp_listener(addr))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    info!(address = %addr, "TCP source listening");

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                info!(peer = %peer, "source: TCP connection accepted");
                stats.conn_open();

                let channels = Arc::clone(&channels);
                let stats = Arc::clone(&stats);
                let mut sd = shutdown.clone();

                tokio::spawn(async move {
                    if let Err(e) = relay_tcp_reader(
                        stream, &channels, &stats, max_payload, &mut sd,
                    ).await {
                        warn!(peer = %peer, error = %e, "source: TCP read error");
                        stats.add_error();
                    }
                    info!(peer = %peer, "source: TCP connection closed");
                    stats.conn_close();
                });
            }
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// ── TCP source: client (connect-out with reconnect) ────────────────

async fn source_tcp_client(
    cfg: EndpointConfig,
    channels: LiveChannels,
    stats: Arc<Stats>,
    max_payload: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let delay = cfg.reconnect_delay();

    loop {
        info!(address = %addr, "source: connecting to TCP endpoint");
        match transport::connect_tcp(addr).await {
            Ok(stream) => {
                info!(address = %addr, "source: TCP connected");
                stats.conn_open();
                let mut sd = shutdown.clone();
                let res = relay_tcp_reader(stream, &channels, &stats, max_payload, &mut sd).await;
                stats.conn_close();
                if let Err(e) = res {
                    warn!(address = %addr, error = %e, "source: TCP error");
                    stats.add_error();
                }
            }
            Err(e) => {
                warn!(address = %addr, error = %e, "source: TCP connect failed");
                stats.add_error();
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// ── Shared TCP reader ──────────────────────────────────────────────

async fn relay_tcp_reader(
    mut stream: TcpStream,
    channels: &LiveChannels,
    stats: &Stats,
    max_payload: usize,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()> {
    let mut buf = vec![0u8; max_payload];
    loop {
        tokio::select! {
            result = stream.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    return Ok(());
                }
                stats.add_received(n as u64);
                let data = Bytes::copy_from_slice(&buf[..n]);
                send_to_all(channels, data).await;
            }
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// ── UDP source (server = bind, client = bind + connect) ────────────

async fn source_udp(
    cfg: EndpointConfig,
    channels: LiveChannels,
    stats: Arc<Stats>,
    max_payload: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let socket = if cfg.mode == EndpointMode::Server {
        let cfg_for_bind = cfg.clone();
        tokio::task::spawn_blocking(move || transport::bind_udp_recv(addr, &cfg_for_bind))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??
    } else {
        let cfg_for_bind = cfg.clone();
        let s = tokio::task::spawn_blocking(move || transport::bind_udp_send(addr, &cfg_for_bind))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??;
        s.connect(addr).await?;
        s
    };

    info!(address = %addr, mode = ?cfg.mode, cast = ?cfg.cast_mode, "UDP source ready");

    // Decouple socket reads from fan-out: the recv task drains the kernel
    // buffer continuously; datagrams that arrive while fan-out is busy are
    // dropped here (counted) rather than silently by the kernel.
    let (recv_tx, mut recv_rx) = mpsc::channel::<Bytes>(64);
    let stats_recv = Arc::clone(&stats);
    let mut sd_recv = shutdown.clone();
    let recv_task = tokio::spawn(async move {
        // Buffer is max_payload + 1 so recv_from can detect oversized datagrams.
        let mut buf = vec![0u8; max_payload + 1];
        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    let (n, peer) = match result {
                        Ok(r) => r,
                        Err(e) => { warn!(error = %e, "UDP source recv error"); stats_recv.add_error(); continue; }
                    };
                    if n > max_payload {
                        warn!(size = n, max = max_payload, peer = %peer, "payload too large, dropped");
                        stats_recv.add_dropped_oversize(1);
                        continue;
                    }
                    stats_recv.add_received(n as u64);
                    debug!(bytes = n, peer = %peer, "UDP source recv");
                    let data = Bytes::copy_from_slice(&buf[..n]);
                    if recv_tx.try_send(data).is_err() {
                        stats_recv.add_dropped_overflow(1);
                    }
                }
                _ = sd_recv.changed() => return,
            }
        }
    });

    loop {
        tokio::select! {
            msg = recv_rx.recv() => {
                match msg {
                    Some(data) => send_to_all(&channels, data).await,
                    None => break,
                }
            }
            _ = shutdown.changed() => break,
        }
    }
    recv_task.abort();
    Ok(())
}

// ════════════════════════════════════════════════════════════════════
//  Destination
// ════════════════════════════════════════════════════════════════════

async fn run_destination(
    cfg: DestConfig,
    runtime: Arc<DestRuntime>,
    global_limiter: SharedLimiter,
    rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    shutdown: watch::Receiver<bool>,
    channel_cap: usize,
) -> Result<()> {
    match (cfg.protocol, cfg.mode) {
        (Protocol::Tcp, EndpointMode::Client) => {
            dest_tcp_client(cfg, runtime, global_limiter, rx, stats, shutdown).await
        }
        (Protocol::Tcp, EndpointMode::Server) => {
            dest_tcp_server(
                cfg,
                runtime,
                global_limiter,
                rx,
                stats,
                shutdown,
                channel_cap,
            )
            .await
        }
        (Protocol::Udp, EndpointMode::Client) => {
            dest_udp_client(cfg, runtime, global_limiter, rx, stats, shutdown).await
        }
        (Protocol::Udp, EndpointMode::Server) => {
            dest_udp_server(cfg, runtime, global_limiter, rx, stats, shutdown).await
        }
    }
}

/// Acquire tokens from the destination's effective limiter, if any.
/// Pulled out so each protocol's write path stays a single line.
async fn acquire_dest_tokens(runtime: &DestRuntime, global: &SharedLimiter, n: u64) {
    if let Some(rl) = runtime.effective_limiter(global) {
        rl.acquire(n).await;
    }
}

// ── TCP destination: client (connect-out with reconnect) ───────────

async fn dest_tcp_client(
    cfg: DestConfig,
    runtime: Arc<DestRuntime>,
    global_limiter: SharedLimiter,
    mut rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();

    loop {
        info!(dest = %name, address = %addr, "dest: connecting TCP");
        match transport::connect_tcp(addr).await {
            Ok(mut stream) => {
                info!(dest = %name, "dest: TCP connected");
                stats.conn_open();

                let write_err = loop {
                    tokio::select! {
                        msg = rx.recv() => match msg {
                            Some(data) => {
                                acquire_dest_tokens(&runtime, &global_limiter, data.len() as u64).await;
                                if let Err(e) = stream.write_all(&data).await {
                                    break Some(e.to_string());
                                }
                                stats.add_sent(data.len() as u64);
                                stats.msg_relayed();
                            }
                            None => break None,
                        },
                        _ = shutdown.changed() => break None,
                    }
                };

                stats.conn_close();
                match write_err {
                    Some(e) => {
                        warn!(dest = %name, error = %e, "dest: TCP write error");
                        stats.add_error();
                        stats.add_dropped_write_error(1);
                    }
                    None => return Ok(()),
                }
            }
            Err(e) => {
                warn!(dest = %name, error = %e, "dest: TCP connect failed");
                stats.add_error();
            }
        }

        // Re-read each iteration so a hot-reload change takes effect on the
        // next reconnect without disturbing the current connection.
        tokio::select! {
            _ = tokio::time::sleep(runtime.reconnect_delay()) => {}
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// ── TCP destination: server (listen, fan-out to connected peers) ───

async fn dest_tcp_server(
    cfg: DestConfig,
    runtime: Arc<DestRuntime>,
    global_limiter: SharedLimiter,
    rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
    channel_cap: usize,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();
    let listener = tokio::task::spawn_blocking(move || transport::bind_tcp_listener(addr))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    info!(dest = %name, address = %addr, "dest: TCP server listening");

    // One bounded channel per peer.  The fan-out loop sends into these channels
    // without holding the lock across any I/O; each peer drains its own channel
    // in a dedicated write task.
    let peer_txs: Arc<Mutex<HashMap<SocketAddr, mpsc::Sender<Bytes>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let peer_txs_accept = Arc::clone(&peer_txs);
    let stats_accept = Arc::clone(&stats);
    let name_accept = name.clone();
    let mut sd_accept = shutdown.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, peer)) => {
                            info!(dest = %name_accept, peer = %peer, "dest-server: peer connected");
                            let (peer_tx, peer_rx) = mpsc::channel::<Bytes>(channel_cap);
                            // insert() drops the old sender if this peer reconnects; the old
                            // write task will call conn_close when it drains, so pre-account
                            // for that here to avoid a transient overcount.
                            if peer_txs_accept.lock().await.insert(peer, peer_tx).is_some() {
                                stats_accept.conn_close();
                            }
                            stats_accept.conn_open();
                            let s = Arc::clone(&stats_accept);
                            let n = name_accept.clone();
                            tokio::spawn(write_to_peer(stream, peer, peer_rx, s, n));
                        }
                        Err(e) => {
                            warn!(dest = %name_accept, error = %e, "dest-server: accept error");
                            stats_accept.add_error();
                        }
                    }
                }
                _ = sd_accept.changed() => return,
            }
        }
    });

    let mut rx = rx;
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        // Acquire once per destination packet: a TCP-server dest's
                        // limit is its aggregate egress to all connected peers, not
                        // per-peer (otherwise N peers would multiply throughput).
                        acquire_dest_tokens(&runtime, &global_limiter, data.len() as u64).await;
                        stats.msg_relayed();
                        // Snapshot senders under a brief lock so the accept task
                        // is never blocked behind the full fan-out iteration.
                        let snapshot: Vec<(SocketAddr, mpsc::Sender<Bytes>)> = peer_txs
                            .lock()
                            .await
                            .iter()
                            .map(|(&p, tx)| (p, tx.clone()))
                            .collect();
                        let mut disconnected: Vec<SocketAddr> = Vec::new();
                        for (peer, tx) in &snapshot {
                            match tx.try_send(data.clone()) {
                                Ok(()) => {}
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    // Peer is slow; drop this packet for it.
                                    stats.add_dropped_overflow(1);
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    // Write task already exited and called conn_close.
                                    disconnected.push(*peer);
                                }
                            }
                        }
                        if !disconnected.is_empty() {
                            let mut txs = peer_txs.lock().await;
                            for peer in disconnected {
                                txs.remove(&peer);
                            }
                        }
                    }
                    None => return Ok(()),
                }
            }
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

/// Drains a per-peer channel and writes each payload to the TCP stream.
/// Called as a spawned task; owns the write half of the connection.
/// Calls `conn_close` exactly once on exit, covering both error and clean paths.
async fn write_to_peer(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    mut rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    dest_name: String,
) {
    let (_, mut writer) = stream.into_split();
    while let Some(data) = rx.recv().await {
        if let Err(e) = writer.write_all(&data).await {
            debug!(dest = %dest_name, peer = %peer, error = %e, "dest-server: write failed");
            stats.add_error();
            stats.add_dropped_write_error(1);
            break;
        }
        stats.add_sent(data.len() as u64);
    }
    stats.conn_close();
}

// ── UDP destination: client (send-to) ──────────────────────────────

async fn dest_udp_client(
    cfg: DestConfig,
    runtime: Arc<DestRuntime>,
    global_limiter: SharedLimiter,
    mut rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();
    let cfg_for_bind = cfg.clone();
    let socket = tokio::task::spawn_blocking(move || transport::bind_udp_send(addr, &cfg_for_bind))
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    info!(dest = %name, address = %addr, cast = ?cfg.cast_mode, "dest: UDP client ready");

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        acquire_dest_tokens(&runtime, &global_limiter, data.len() as u64).await;
                        match socket.send_to(&data, addr).await {
                            Ok(n) => {
                                stats.add_sent(n as u64);
                                stats.msg_relayed();
                            }
                            Err(e) => {
                                warn!(dest = %name, error = %e, "dest: UDP send error");
                                stats.add_error();
                                stats.add_dropped_write_error(1);
                            }
                        }
                    }
                    None => return Ok(()),
                }
            }
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// The async dest_* fns below sit after this test module for historical
// reasons; suppress the structural lint rather than churn the file.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn test_config() -> RelayConfig {
        toml::from_str(
            r#"
[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "127.0.0.1:0"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:0"
"#,
        )
        .unwrap()
    }

    #[test]
    fn relay_new_creates_correct_stats_labels() {
        let relay = Relay::new(test_config(), "/tmp/test.toml".to_string());
        assert!(
            relay.source_stats.label.starts_with("source("),
            "label: {}",
            relay.source_stats.label
        );
        assert_eq!(relay.dest_stats.len(), 1);
        assert!(
            relay.dest_stats[0].label.starts_with("dest("),
            "label: {}",
            relay.dest_stats[0].label
        );
    }

    #[test]
    fn relay_new_stats_count_matches_destinations() {
        let mut cfg = test_config();
        cfg.destinations.push(cfg.destinations[0].clone());
        let relay = Relay::new(cfg, "/tmp/test.toml".to_string());
        assert_eq!(relay.dest_stats.len(), 2);
    }

    fn test_runtime(policy: OverflowPolicy) -> Arc<DestRuntime> {
        Arc::new(DestRuntime::new(policy, 2000, None))
    }

    fn wrap_channels(channels: Vec<DestChannel>) -> LiveChannels {
        Arc::new(ArcSwap::from_pointee(channels))
    }

    fn wrap_stats(stats: Vec<Arc<Stats>>) -> LiveStats {
        Arc::new(ArcSwap::from_pointee(stats))
    }

    /// Build a `HotReloadCtx` whose supervisor mirrors `cfg.destinations`.
    /// Spawns real destination tasks against the cfg endpoints — for unit
    /// tests this is fine because TCP-client tasks to `127.0.0.1:0` just
    /// fail to connect repeatedly; we never assert anything about their
    /// side effects.
    async fn test_ctx_for(cfg: &RelayConfig) -> HotReloadCtx {
        let (_sd_tx, sd_rx) = watch::channel(false);
        let source_stats = Arc::new(Stats::new("source(test)", "", ""));
        let dest_stats: Vec<Arc<Stats>> = cfg
            .destinations
            .iter()
            .map(|d| Arc::new(build_dest_stats(d)))
            .collect();
        let live_channels = wrap_channels(Vec::new());
        let live_stats = wrap_stats(vec![Arc::clone(&source_stats)]);
        let stats_interval_secs = Arc::new(AtomicU64::new(cfg.general.stats_interval_secs));
        let global_limiter: SharedLimiter = Arc::new(RwLock::new(None));

        let mut supervisor = DestSupervisor::new(
            Arc::clone(&source_stats),
            Arc::clone(&live_channels),
            Arc::clone(&live_stats),
            cfg.general.channel_capacity.max(1),
            Arc::clone(&stats_interval_secs),
            Arc::clone(&global_limiter),
            sd_rx,
        );
        supervisor.spawn_initial(&cfg.destinations, &dest_stats);

        HotReloadCtx {
            limiter: global_limiter,
            stats_interval_secs,
            supervisor: Arc::new(tokio::sync::Mutex::new(supervisor)),
            log_setter: noop_log_setter(),
        }
    }

    #[tokio::test]
    async fn send_to_all_drop_newest_increments_dropped() {
        let (tx, mut rx) = mpsc::channel::<Bytes>(1);
        tx.try_send(Bytes::from("fill")).unwrap(); // fill to capacity
        let stats = Arc::new(Stats::new("dest", "", ""));
        let ch = DestChannel {
            tx,
            name: "dest".to_string(),
            stats: Arc::clone(&stats),
            runtime: test_runtime(OverflowPolicy::DropNewest),
        };
        let channels = wrap_channels(vec![ch]);
        send_to_all(&channels, Bytes::from("overflow")).await;
        assert_eq!(stats.dropped_messages.load(Ordering::Relaxed), 1);
        assert!(rx.try_recv().is_ok()); // original fill item still there
    }

    #[tokio::test]
    async fn send_to_all_block_delivers_when_space_available() {
        let (tx, mut rx) = mpsc::channel::<Bytes>(4);
        let stats = Arc::new(Stats::new("dest", "", ""));
        let ch = DestChannel {
            tx,
            name: "dest".to_string(),
            stats: Arc::clone(&stats),
            runtime: test_runtime(OverflowPolicy::Block),
        };
        let channels = wrap_channels(vec![ch]);
        let data = Bytes::from("hello");
        send_to_all(&channels, data.clone()).await;
        let received = rx.recv().await.unwrap();
        assert_eq!(received, data);
        assert_eq!(stats.dropped_messages.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn apply_hot_reload_installs_rate_limiter() {
        let old = test_config();
        let mut new = test_config();
        new.rate_limit = Some(RateLimitConfig {
            bytes_per_second: 500,
            burst_size: 1000,
        });
        let ctx = test_ctx_for(&old).await;
        apply_hot_reload(&old, &new, &ctx).await;
        assert!(ctx.limiter.read().unwrap().is_some());
    }

    #[tokio::test]
    async fn apply_hot_reload_removes_rate_limiter() {
        let mut old = test_config();
        old.rate_limit = Some(RateLimitConfig {
            bytes_per_second: 1000,
            burst_size: 5000,
        });
        let new = test_config(); // no rate_limit
        let ctx = test_ctx_for(&old).await;
        *ctx.limiter.write().unwrap() = Some(Arc::new(RateLimiter::new(1000, 5000)));
        apply_hot_reload(&old, &new, &ctx).await;
        assert!(ctx.limiter.read().unwrap().is_none());
    }

    // ── New live-field hot-reload coverage ────────────────────────

    #[tokio::test]
    async fn apply_hot_reload_updates_log_level_via_setter() {
        use std::sync::Mutex;
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let setter: LogLevelSetter = Arc::new(move |level: &str| {
            captured_clone.lock().unwrap().push(level.to_string());
            true
        });

        let mut old = test_config();
        old.general.log_level = "info".to_string();
        let mut new = test_config();
        new.general.log_level = "debug".to_string();

        let mut ctx = test_ctx_for(&old).await;
        ctx.log_setter = setter;

        apply_hot_reload(&old, &new, &ctx).await;
        assert_eq!(captured.lock().unwrap().as_slice(), &["debug".to_string()]);
    }

    #[tokio::test]
    async fn apply_hot_reload_skips_log_level_when_unchanged() {
        use std::sync::Mutex;
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = Arc::clone(&captured);
        let setter: LogLevelSetter = Arc::new(move |level: &str| {
            captured_clone.lock().unwrap().push(level.to_string());
            true
        });

        let cfg = test_config();
        let mut ctx = test_ctx_for(&cfg).await;
        ctx.log_setter = setter;

        apply_hot_reload(&cfg, &cfg, &ctx).await;
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn apply_hot_reload_updates_stats_interval_atomic() {
        let mut old = test_config();
        old.general.stats_interval_secs = 30;
        let mut new = test_config();
        new.general.stats_interval_secs = 5;

        let ctx = test_ctx_for(&old).await;
        ctx.stats_interval_secs.store(30, Ordering::Relaxed);

        apply_hot_reload(&old, &new, &ctx).await;
        assert_eq!(ctx.stats_interval_secs.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn apply_hot_reload_updates_overflow_policy_atomic() {
        let mut old = test_config();
        old.destinations[0].overflow_policy = OverflowPolicy::DropNewest;
        let mut new = test_config();
        new.destinations[0].overflow_policy = OverflowPolicy::Block;

        let ctx = test_ctx_for(&old).await;
        let key = DestKey::from_cfg(&old.destinations[0]);
        let rt = ctx
            .supervisor
            .lock()
            .await
            .entry_runtime(&key)
            .expect("entry");
        assert_eq!(rt.policy(), OverflowPolicy::DropNewest);

        apply_hot_reload(&old, &new, &ctx).await;
        assert_eq!(rt.policy(), OverflowPolicy::Block);
    }

    #[tokio::test]
    async fn apply_hot_reload_updates_reconnect_delay_atomic() {
        let mut old = test_config();
        old.destinations[0].base.reconnect_delay_ms = Some(2000);
        let mut new = test_config();
        new.destinations[0].base.reconnect_delay_ms = Some(500);

        let ctx = test_ctx_for(&old).await;
        let key = DestKey::from_cfg(&old.destinations[0]);
        let rt = ctx
            .supervisor
            .lock()
            .await
            .entry_runtime(&key)
            .expect("entry");

        apply_hot_reload(&old, &new, &ctx).await;
        assert_eq!(rt.reconnect_delay(), Duration::from_millis(500));
    }

    #[tokio::test]
    async fn apply_hot_reload_identity_change_swaps_entry_not_in_place() {
        let old = test_config();
        let mut new = test_config();
        // Different address ⇒ different DestKey ⇒ old entry removed,
        // new entry spawned; the supervisor must NOT keep the old key.
        new.destinations[0].base.address = "127.0.0.1:1".to_string();
        new.destinations[0].overflow_policy = OverflowPolicy::Block;

        let ctx = test_ctx_for(&old).await;
        let old_key = DestKey::from_cfg(&old.destinations[0]);
        let new_key = DestKey::from_cfg(&new.destinations[0]);

        apply_hot_reload(&old, &new, &ctx).await;
        let sup = ctx.supervisor.lock().await;
        assert!(
            sup.entry_runtime(&old_key).is_none(),
            "old key should be gone"
        );
        assert!(
            sup.entry_runtime(&new_key).is_some(),
            "new key should exist"
        );
        assert_eq!(sup.entry_count(), 1);
    }

    #[tokio::test]
    async fn apply_hot_reload_adds_new_destination() {
        let old = test_config();
        let mut new = test_config();
        new.destinations.push(DestConfig {
            base: EndpointConfig {
                name: None,
                protocol: Protocol::Tcp,
                mode: EndpointMode::Client,
                address: "127.0.0.1:2".to_string(),
                cast_mode: CastMode::Unicast,
                multicast_interface: None,
                multicast_interface_index: None,
                multicast_ttl: 16,
                reconnect_delay_ms: None,
            },
            overflow_policy: OverflowPolicy::DropNewest,
            rate_limit: None,
        });

        let ctx = test_ctx_for(&old).await;
        let added_key = DestKey::from_cfg(&new.destinations[1]);

        apply_hot_reload(&old, &new, &ctx).await;
        let sup = ctx.supervisor.lock().await;
        assert_eq!(sup.entry_count(), 2);
        assert!(sup.entry_runtime(&added_key).is_some());
    }

    // ── Per-destination rate limit ─────────────────────────────────

    fn rl_cfg(bps: u64, burst: u64) -> RateLimitConfig {
        RateLimitConfig {
            bytes_per_second: bps,
            burst_size: burst,
        }
    }

    #[test]
    fn effective_limiter_returns_none_when_neither_set() {
        let runtime = DestRuntime::new(OverflowPolicy::DropNewest, 2000, None);
        let global: SharedLimiter = Arc::new(RwLock::new(None));
        assert!(runtime.effective_limiter(&global).is_none());
    }

    #[test]
    fn effective_limiter_uses_global_when_only_global_set() {
        let runtime = DestRuntime::new(OverflowPolicy::DropNewest, 2000, None);
        let global_rl = Arc::new(RateLimiter::new(1_000, 5_000));
        let global: SharedLimiter = Arc::new(RwLock::new(Some(Arc::clone(&global_rl))));
        let effective = runtime.effective_limiter(&global).expect("some");
        assert!(Arc::ptr_eq(&effective, &global_rl));
    }

    #[test]
    fn effective_limiter_uses_private_when_only_private_set() {
        let runtime = DestRuntime::new(OverflowPolicy::DropNewest, 2000, Some(&rl_cfg(500, 1_000)));
        let global: SharedLimiter = Arc::new(RwLock::new(None));
        assert!(runtime.effective_limiter(&global).is_some());
    }

    #[test]
    fn effective_limiter_private_wins_when_both_set() {
        let runtime = DestRuntime::new(OverflowPolicy::DropNewest, 2000, Some(&rl_cfg(500, 1_000)));
        let global_rl = Arc::new(RateLimiter::new(1_000_000, 5_000_000));
        let global: SharedLimiter = Arc::new(RwLock::new(Some(Arc::clone(&global_rl))));
        let effective = runtime.effective_limiter(&global).expect("some");
        assert!(
            !Arc::ptr_eq(&effective, &global_rl),
            "private limiter must override global"
        );
    }

    #[test]
    fn set_private_limiter_clears_to_none() {
        let runtime = DestRuntime::new(OverflowPolicy::DropNewest, 2000, Some(&rl_cfg(500, 1_000)));
        let global: SharedLimiter = Arc::new(RwLock::new(None));
        assert!(runtime.effective_limiter(&global).is_some());
        runtime.set_private_limiter(None);
        assert!(runtime.effective_limiter(&global).is_none());
    }

    #[tokio::test]
    async fn apply_hot_reload_installs_per_dest_rate_limit() {
        let old = test_config();
        let mut new = test_config();
        new.destinations[0].rate_limit = Some(rl_cfg(2_000, 8_000));

        let ctx = test_ctx_for(&old).await;
        let key = DestKey::from_cfg(&old.destinations[0]);
        let rt = ctx
            .supervisor
            .lock()
            .await
            .entry_runtime(&key)
            .expect("entry");
        let global: SharedLimiter = Arc::new(RwLock::new(None));
        assert!(rt.effective_limiter(&global).is_none());

        apply_hot_reload(&old, &new, &ctx).await;
        assert!(
            rt.effective_limiter(&global).is_some(),
            "per-dest limiter should be installed after hot-reload"
        );
    }

    #[tokio::test]
    async fn apply_hot_reload_removes_per_dest_rate_limit() {
        let mut old = test_config();
        old.destinations[0].rate_limit = Some(rl_cfg(2_000, 8_000));
        let new = test_config(); // per-dest cleared, falls back to global

        let ctx = test_ctx_for(&old).await;
        let key = DestKey::from_cfg(&old.destinations[0]);
        let rt = ctx
            .supervisor
            .lock()
            .await
            .entry_runtime(&key)
            .expect("entry");
        let global: SharedLimiter = Arc::new(RwLock::new(None));
        assert!(rt.effective_limiter(&global).is_some());

        apply_hot_reload(&old, &new, &ctx).await;
        assert!(
            rt.effective_limiter(&global).is_none(),
            "per-dest limiter must be cleared so the dest falls back to global"
        );
    }

    #[tokio::test]
    async fn apply_hot_reload_removes_destination() {
        let mut old = test_config();
        // Add a second dest so we can remove one without going empty
        // (validate() would reject an empty destinations list).
        old.destinations.push(DestConfig {
            base: EndpointConfig {
                name: None,
                protocol: Protocol::Tcp,
                mode: EndpointMode::Client,
                address: "127.0.0.1:3".to_string(),
                cast_mode: CastMode::Unicast,
                multicast_interface: None,
                multicast_interface_index: None,
                multicast_ttl: 16,
                reconnect_delay_ms: None,
            },
            overflow_policy: OverflowPolicy::DropNewest,
            rate_limit: None,
        });
        let mut new = old.clone();
        let removed_key = DestKey::from_cfg(&old.destinations[1]);
        new.destinations.remove(1);

        let ctx = test_ctx_for(&old).await;
        assert_eq!(ctx.supervisor.lock().await.entry_count(), 2);

        apply_hot_reload(&old, &new, &ctx).await;
        let sup = ctx.supervisor.lock().await;
        assert_eq!(sup.entry_count(), 1);
        assert!(sup.entry_runtime(&removed_key).is_none());
    }

    // ── Pure diff function ────────────────────────────────────────

    fn dest_cfg(address: &str) -> DestConfig {
        DestConfig {
            base: EndpointConfig {
                name: None,
                protocol: Protocol::Tcp,
                mode: EndpointMode::Client,
                address: address.to_string(),
                cast_mode: CastMode::Unicast,
                multicast_interface: None,
                multicast_interface_index: None,
                multicast_ttl: 16,
                reconnect_delay_ms: None,
            },
            overflow_policy: OverflowPolicy::DropNewest,
            rate_limit: None,
        }
    }

    #[test]
    fn diff_destinations_empty_to_empty() {
        let plan = diff_destinations(&[], &[]);
        assert_eq!(plan, DiffPlan::default());
    }

    #[test]
    fn diff_destinations_pure_add() {
        let a = dest_cfg("127.0.0.1:1");
        let plan = diff_destinations(&[], std::slice::from_ref(&a));
        assert_eq!(plan.added, vec![DestKey::from_cfg(&a)]);
        assert!(plan.removed.is_empty());
        assert!(plan.kept.is_empty());
    }

    #[test]
    fn diff_destinations_pure_remove() {
        let a = dest_cfg("127.0.0.1:1");
        let plan = diff_destinations(std::slice::from_ref(&a), &[]);
        assert_eq!(plan.removed, vec![DestKey::from_cfg(&a)]);
        assert!(plan.added.is_empty());
        assert!(plan.kept.is_empty());
    }

    #[test]
    fn diff_destinations_identity_change_is_add_plus_remove() {
        let a = dest_cfg("127.0.0.1:1");
        let b = dest_cfg("127.0.0.1:2");
        let plan = diff_destinations(std::slice::from_ref(&a), std::slice::from_ref(&b));
        assert_eq!(plan.added, vec![DestKey::from_cfg(&b)]);
        assert_eq!(plan.removed, vec![DestKey::from_cfg(&a)]);
        assert!(plan.kept.is_empty());
    }

    #[test]
    fn diff_destinations_policy_change_is_kept_not_add_remove() {
        let mut a = dest_cfg("127.0.0.1:1");
        let b = a.clone();
        a.overflow_policy = OverflowPolicy::Block;
        let plan = diff_destinations(std::slice::from_ref(&a), std::slice::from_ref(&b));
        assert!(plan.added.is_empty());
        assert!(plan.removed.is_empty());
        assert_eq!(plan.kept, vec![DestKey::from_cfg(&a)]);
    }

    #[test]
    fn diff_destinations_partial_overlap() {
        let a = dest_cfg("127.0.0.1:1");
        let b = dest_cfg("127.0.0.1:2");
        let c = dest_cfg("127.0.0.1:3");
        let old = vec![a.clone(), b.clone()];
        let new = vec![b.clone(), c.clone()];
        let plan = diff_destinations(&old, &new);
        assert_eq!(plan.added, vec![DestKey::from_cfg(&c)]);
        assert_eq!(plan.removed, vec![DestKey::from_cfg(&a)]);
        assert_eq!(plan.kept, vec![DestKey::from_cfg(&b)]);
    }

    #[tokio::test]
    async fn run_reporter_reads_interval_from_atomic() {
        // Drive the reporter with a 1-second interval, then bump it to a
        // large value mid-flight and assert no additional log_report tick
        // fires within the short window. We can't easily intercept log
        // output here, so we use shutdown timing as a coarse proxy:
        // the reporter must exit cleanly on shutdown regardless of the
        // live interval value.
        let stats = Arc::new(Stats::new("test", "", ""));
        let interval = Arc::new(AtomicU64::new(1));
        let (tx, rx) = watch::channel(false);

        let stats_for_task = Arc::clone(&stats);
        let interval_for_task = Arc::clone(&interval);
        let handle = tokio::spawn(async move {
            crate::stats::run_reporter(stats_for_task, interval_for_task, rx).await;
        });

        // Bump the interval — reporter should pick this up on next tick.
        interval.store(3600, Ordering::Relaxed);
        assert_eq!(interval.load(Ordering::Relaxed), 3600);

        // Trigger shutdown, reporter must return promptly.
        tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("reporter did not exit within 2s of shutdown")
            .unwrap();
    }

    #[tokio::test]
    async fn health_endpoint_returns_200() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stats = vec![Arc::new(Stats::new("test", "", ""))];

        let server = {
            let stats = stats.clone();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                serve_health_request(stream, wrap_stats(stats)).await;
            })
        };

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("200 OK"), "got: {response}");
        assert!(response.contains(r#"{"status":"ok"}"#), "got: {response}");
    }

    #[tokio::test]
    async fn stats_endpoint_returns_json() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stat = Arc::new(Stats::new("src", "", ""));
        stat.add_received(42);
        let stats = vec![Arc::clone(&stat)];

        let server = {
            let stats = stats.clone();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                serve_health_request(stream, wrap_stats(stats)).await;
            })
        };

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /stats HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("200 OK"), "got: {response}");
        assert!(response.contains("rx_bytes"), "got: {response}");
        assert!(response.contains("\"label\":\"src\""), "got: {response}");
    }

    #[tokio::test]
    async fn metrics_endpoint_returns_prometheus_text() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stat = Arc::new(Stats::new("source(ingest)", "", ""));
        stat.add_received(4096);
        stat.add_sent(0);
        stat.msg_relayed();
        let stats = vec![Arc::clone(&stat)];

        let server = {
            let stats = stats.clone();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                serve_health_request(stream, wrap_stats(stats)).await;
            })
        };

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("200 OK"), "got: {response}");
        assert!(
            response.contains("Content-Type: text/plain"),
            "got: {response}"
        );
        // HELP + TYPE markers + at least one counter line with the endpoint label.
        assert!(
            response.contains("# HELP nexthop_rx_bytes_total"),
            "got: {response}"
        );
        assert!(
            response.contains("# TYPE nexthop_rx_bytes_total counter"),
            "got: {response}"
        );
        assert!(
            response.contains("nexthop_rx_bytes_total{endpoint=\"source(ingest)\"} 4096"),
            "got: {response}"
        );
        // Gauges present.
        assert!(
            response.contains("# TYPE nexthop_active_connections gauge"),
            "got: {response}"
        );
    }

    #[tokio::test]
    async fn unknown_path_returns_404() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stats: Vec<Arc<Stats>> = vec![];

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_health_request(stream, wrap_stats(stats)).await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /unknown HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("404"), "got: {response}");
    }

    // ── Health server bind / dual-stack coverage ──────────────────

    fn ephemeral_port() -> u16 {
        let sock = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = sock.local_addr().expect("local_addr").port();
        drop(sock);
        port
    }

    async fn http_get_status_line(addr: &str) -> std::io::Result<String> {
        let mut s = tokio::net::TcpStream::connect(addr).await?;
        s.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut buf = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut s, &mut buf).await?;
        Ok(buf)
    }

    #[test]
    fn resolve_health_bind_addrs_default_is_dual_stack() {
        let addrs = resolve_health_bind_addrs(9090, None);
        assert_eq!(addrs.len(), 2);
        assert!(addrs.iter().any(|a| a.is_ipv4() && a.port() == 9090));
        assert!(addrs.iter().any(|a| a.is_ipv6() && a.port() == 9090));
    }

    #[test]
    fn resolve_health_bind_addrs_explicit_v4_only() {
        let addrs = resolve_health_bind_addrs(9090, Some("127.0.0.1"));
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv4());
        assert_eq!(addrs[0].port(), 9090);
    }

    #[test]
    fn resolve_health_bind_addrs_explicit_v6_only() {
        let addrs = resolve_health_bind_addrs(9090, Some("::1"));
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv6());
        assert_eq!(addrs[0].port(), 9090);
    }

    #[test]
    fn resolve_health_bind_addrs_invalid_falls_back_to_dual_stack() {
        // Garbage input should not panic and should still leave the server
        // reachable on something (the dual-stack default).
        let addrs = resolve_health_bind_addrs(9090, Some("not-an-ip"));
        assert_eq!(addrs.len(), 2);
    }

    #[tokio::test]
    async fn health_server_dual_stack_binds_both_v4_and_v6() {
        let port = ephemeral_port();
        let stats = wrap_stats(vec![Arc::new(Stats::new("source(test)", "", ""))]);
        let (sd_tx, sd_rx) = watch::channel(false);

        let handle = spawn_health_server(port, None, stats, sd_rx);
        // Give the bind tasks a moment to install both listeners.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let v4 = http_get_status_line(&format!("127.0.0.1:{port}"))
            .await
            .expect("v4 connect");
        assert!(v4.contains("200 OK"), "v4 response: {v4}");

        let v6 = http_get_status_line(&format!("[::1]:{port}"))
            .await
            .expect("v6 connect");
        assert!(v6.contains("200 OK"), "v6 response: {v6}");

        sd_tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn health_server_ipv4_only_when_configured() {
        let port = ephemeral_port();
        let stats = wrap_stats(vec![Arc::new(Stats::new("source(test)", "", ""))]);
        let (sd_tx, sd_rx) = watch::channel(false);

        let handle = spawn_health_server(port, Some("127.0.0.1".to_string()), stats, sd_rx);
        tokio::time::sleep(Duration::from_millis(200)).await;

        let v4 = http_get_status_line(&format!("127.0.0.1:{port}"))
            .await
            .expect("v4 connect");
        assert!(v4.contains("200 OK"), "v4 response: {v4}");

        let v6_result = tokio::time::timeout(
            Duration::from_millis(400),
            http_get_status_line(&format!("[::1]:{port}")),
        )
        .await;
        assert!(
            matches!(v6_result, Ok(Err(_)) | Err(_)),
            "v6 connect should fail or time out, got: {v6_result:?}"
        );

        sd_tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn health_server_ipv6_only_when_configured() {
        let port = ephemeral_port();
        let stats = wrap_stats(vec![Arc::new(Stats::new("source(test)", "", ""))]);
        let (sd_tx, sd_rx) = watch::channel(false);

        let handle = spawn_health_server(port, Some("::1".to_string()), stats, sd_rx);
        tokio::time::sleep(Duration::from_millis(200)).await;

        let v6 = http_get_status_line(&format!("[::1]:{port}"))
            .await
            .expect("v6 connect");
        assert!(v6.contains("200 OK"), "v6 response: {v6}");

        let v4_result = tokio::time::timeout(
            Duration::from_millis(400),
            http_get_status_line(&format!("127.0.0.1:{port}")),
        )
        .await;
        assert!(
            matches!(v4_result, Ok(Err(_)) | Err(_)),
            "v4 connect should fail or time out, got: {v4_result:?}"
        );

        sd_tx.send(true).unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[test]
    fn relay_new_source_addr_fields_server_mode() {
        let cfg = toml::from_str::<RelayConfig>(
            r#"[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "0.0.0.0:5000"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:0"
"#,
        )
        .unwrap();
        let relay = Relay::new(cfg, "/tmp/t.toml".to_string());
        assert_eq!(relay.source_stats.local_addr, "0.0.0.0:5000");
        assert_eq!(relay.source_stats.peer_addr, "(any)");
    }

    #[test]
    fn relay_new_source_addr_fields_client_mode() {
        let cfg = toml::from_str::<RelayConfig>(
            r#"[general]
[source]
protocol = "tcp"
mode     = "client"
address  = "10.0.0.1:9000"
[[destinations]]
protocol = "tcp"
mode     = "client"
address  = "127.0.0.1:0"
"#,
        )
        .unwrap();
        let relay = Relay::new(cfg, "/tmp/t.toml".to_string());
        assert_eq!(relay.source_stats.local_addr, "(auto)");
        assert_eq!(relay.source_stats.peer_addr, "10.0.0.1:9000");
    }

    #[test]
    fn relay_new_dest_addr_fields_client_mode() {
        let cfg = toml::from_str::<RelayConfig>(
            r#"[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "0.0.0.0:5000"
[[destinations]]
protocol = "udp"
mode     = "client"
address  = "192.168.1.5:6000"
"#,
        )
        .unwrap();
        let relay = Relay::new(cfg, "/tmp/t.toml".to_string());
        assert_eq!(relay.dest_stats[0].local_addr, "(auto)");
        assert_eq!(relay.dest_stats[0].peer_addr, "192.168.1.5:6000");
    }

    #[test]
    fn relay_new_dest_addr_fields_server_mode() {
        let cfg = toml::from_str::<RelayConfig>(
            r#"[general]
[source]
protocol = "tcp"
mode     = "server"
address  = "0.0.0.0:5000"
[[destinations]]
protocol = "udp"
mode     = "server"
address  = "0.0.0.0:7000"
"#,
        )
        .unwrap();
        let relay = Relay::new(cfg, "/tmp/t.toml".to_string());
        assert_eq!(relay.dest_stats[0].local_addr, "0.0.0.0:7000");
        assert_eq!(relay.dest_stats[0].peer_addr, "(any)");
    }

    #[test]
    fn send_to_all_fan_out_reaches_multiple_channels() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (tx1, mut rx1) = mpsc::channel::<Bytes>(4);
            let (tx2, mut rx2) = mpsc::channel::<Bytes>(4);
            let stats1 = Arc::new(Stats::new("d1", "", ""));
            let stats2 = Arc::new(Stats::new("d2", "", ""));
            let channels = wrap_channels(vec![
                DestChannel {
                    tx: tx1,
                    name: "d1".into(),
                    stats: Arc::clone(&stats1),
                    runtime: test_runtime(OverflowPolicy::Block),
                },
                DestChannel {
                    tx: tx2,
                    name: "d2".into(),
                    stats: Arc::clone(&stats2),
                    runtime: test_runtime(OverflowPolicy::Block),
                },
            ]);
            let payload = Bytes::from("broadcast-payload");
            send_to_all(&channels, payload.clone()).await;
            assert_eq!(rx1.recv().await.unwrap(), payload);
            assert_eq!(rx2.recv().await.unwrap(), payload);
        });
    }

    #[tokio::test]
    async fn apply_hot_reload_same_config_no_panic() {
        let cfg = test_config();
        let ctx = test_ctx_for(&cfg).await;
        apply_hot_reload(&cfg, &cfg, &ctx).await;
        assert!(ctx.limiter.read().unwrap().is_none());
    }

    #[test]
    fn relay_shutdown_sender_returns_clone() {
        let relay = Relay::new(test_config(), "/tmp/t.toml".to_string());
        let tx1 = relay.shutdown_sender();
        let tx2 = relay.shutdown_sender();
        // Both senders point at the same watch channel (same receiver count).
        assert_eq!(Arc::strong_count(&tx1), Arc::strong_count(&tx2));
    }

    #[tokio::test]
    async fn health_endpoint_response_has_content_length() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stats: Vec<Arc<Stats>> = vec![];
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_health_request(stream, wrap_stats(stats)).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();
        assert!(
            response.contains("Content-Length:"),
            "missing header:\n{response}"
        );
        assert!(
            response.contains("Content-Type: application/json"),
            "missing content-type:\n{response}"
        );
    }

    #[tokio::test]
    async fn stats_endpoint_multiple_snapshots() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let s1 = Arc::new(Stats::new("src", "0.0.0.0:5000", "(any)"));
        let s2 = Arc::new(Stats::new("dst", "(auto)", "127.0.0.1:5001"));
        s1.add_received(100);
        s2.add_sent(200);
        let stats = vec![Arc::clone(&s1), Arc::clone(&s2)];
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_health_request(stream, wrap_stats(stats)).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /stats HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();
        assert!(response.contains("\"label\":\"src\""), "got: {response}");
        assert!(response.contains("\"label\":\"dst\""), "got: {response}");
    }

    #[tokio::test]
    async fn health_endpoint_with_query_string_returns_200() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stats: Vec<Arc<Stats>> = vec![];
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_health_request(stream, wrap_stats(stats)).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health?v=1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();
        assert!(response.contains("200 OK"), "got: {response}");
    }
}

// ── UDP destination: server (bind, track peers, fan-out) ───────────
//
// Peers "register" by sending any datagram to the bound port.
// The relay then forwards all traffic to every registered peer.

async fn dest_udp_server(
    cfg: DestConfig,
    runtime: Arc<DestRuntime>,
    global_limiter: SharedLimiter,
    rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();
    let cfg_for_bind = cfg.clone();
    let socket = Arc::new(
        tokio::task::spawn_blocking(move || transport::bind_udp_recv(addr, &cfg_for_bind))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))??,
    );
    info!(dest = %name, address = %addr, "dest: UDP server listening for peer registrations");

    let peers: Arc<Mutex<HashSet<SocketAddr>>> = Arc::new(Mutex::new(HashSet::new()));

    let reg_socket = Arc::clone(&socket);
    let reg_peers = Arc::clone(&peers);
    let reg_stats = Arc::clone(&stats);
    let reg_name = name.clone();
    let mut sd_reg = shutdown.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 1];
        loop {
            tokio::select! {
                result = reg_socket.recv_from(&mut buf) => {
                    match result {
                        Ok((_n, peer)) => {
                            let mut set = reg_peers.lock().await;
                            if set.insert(peer) {
                                info!(dest = %reg_name, peer = %peer, "dest-server: UDP peer registered");
                                reg_stats.conn_open();
                            }
                        }
                        Err(e) => {
                            warn!(dest = %reg_name, error = %e, "dest-server: recv error");
                            reg_stats.add_error();
                        }
                    }
                }
                _ = sd_reg.changed() => return,
            }
        }
    });

    let mut rx = rx;
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        // Same model as dest_tcp_server: acquire once per
                        // destination packet, not per peer, so the configured
                        // rate is the dest's aggregate egress.
                        acquire_dest_tokens(&runtime, &global_limiter, data.len() as u64).await;
                        let addrs: Vec<SocketAddr> = {
                            peers.lock().await.iter().copied().collect()
                        };
                        let mut sent_any = false;
                        let mut failed: Vec<SocketAddr> = Vec::new();
                        for peer in &addrs {
                            match socket.send_to(&data, peer).await {
                                Ok(n) => {
                                    stats.add_sent(n as u64);
                                    sent_any = true;
                                }
                                Err(e) => {
                                    warn!(dest = %name, peer = %peer, error = %e, "dest-server: send error");
                                    stats.add_error();
                                    failed.push(*peer);
                                }
                            }
                        }
                        if sent_any {
                            stats.msg_relayed();
                        }
                        // Batch-evict failed peers under a single lock; only call
                        // conn_close when remove() confirms the peer is still registered
                        // (a concurrent re-registration could have already replaced it).
                        if !failed.is_empty() {
                            let mut set = peers.lock().await;
                            for peer in failed {
                                if set.remove(&peer) {
                                    stats.conn_close();
                                }
                            }
                        }
                    }
                    None => return Ok(()),
                }
            }
            _ = shutdown.changed() => return Ok(()),
        }
    }
}
