// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present nexthop@krypte.me
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: nexthop@krypte.me
// Built by:  Anthropic Claude (Sonnet 4.6)

//! Relay engine: wires sources to destinations via per-destination mpsc channels.

use crate::config::*;
use crate::error::Result;
use crate::rate_limiter::RateLimiter;
use crate::stats::Stats;
use crate::transport;
use bytes::Bytes;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Mutex};
use tracing::{debug, error, info, warn};

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

struct DestChannel {
    tx: mpsc::Sender<Bytes>,
    policy: OverflowPolicy,
    name: String,
    stats: Arc<Stats>,
}

/// Fan-out one packet to every destination, applying each destination's
/// overflow policy independently.
async fn send_to_all(channels: &[DestChannel], data: Bytes) {
    let mut block_sends = Vec::new();
    for ch in channels {
        match ch.policy {
            OverflowPolicy::DropNewest => {
                if ch.tx.try_send(data.clone()).is_err() {
                    warn!(dest = %ch.name, "dest queue full, packet dropped");
                    ch.stats.add_dropped(1);
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

pub struct Relay {
    config_path: String,
    config: RelayConfig,
    limiter: SharedLimiter,
    pub source_stats: Arc<Stats>,
    pub dest_stats: Vec<Arc<Stats>>,
    shutdown_tx: Arc<watch::Sender<bool>>,
    shutdown_rx: watch::Receiver<bool>,
}

impl Relay {
    pub fn new(config: RelayConfig, config_path: String) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let limiter_val = config.rate_limit.as_ref().map(|rl| {
            Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size))
        });
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
        let dest_stats = config
            .destinations
            .iter()
            .map(|d| {
                let local = if d.mode == EndpointMode::Server {
                    d.address.clone()
                } else {
                    "(auto)".to_string()
                };
                let peer = if d.mode == EndpointMode::Client {
                    d.address.clone()
                } else {
                    "(any)".to_string()
                };
                Arc::new(Stats::new(
                    format!("dest({})", d.display_name()),
                    local,
                    peer,
                ))
            })
            .collect();
        Self {
            config_path,
            config,
            limiter: Arc::new(RwLock::new(limiter_val)),
            source_stats,
            dest_stats,
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
        let interval = std::time::Duration::from_secs(self.config.general.stats_interval_secs);

        let mut task_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        // Per-source stats (pre-created in new() so the GUI can clone them before run())
        let src_name = self.config.source.display_name();
        let source_stats = Arc::clone(&self.source_stats);
        task_handles.push(tokio::spawn(crate::stats::run_reporter(
            Arc::clone(&source_stats),
            interval,
            self.shutdown_rx.clone(),
        )));

        let mut all_stats: Vec<Arc<Stats>> = vec![Arc::clone(&source_stats)];

        // Config hot-reload watcher — wrap startup config so the watcher can
        // track the last-reloaded baseline rather than comparing forever against
        // the original startup snapshot.
        let current_cfg = Arc::new(RwLock::new(self.config.clone()));
        task_handles.push(spawn_config_watcher(
            self.config_path.clone(),
            current_cfg,
            Arc::clone(&self.limiter),
            self.shutdown_rx.clone(),
        ));

        // Create one mpsc channel + one Stats instance per destination.
        let mut dest_channels: Vec<DestChannel> = Vec::with_capacity(self.config.destinations.len());
        for (idx, dest) in self.config.destinations.iter().enumerate() {
            let (tx, rx) = mpsc::channel::<Bytes>(cap);
            let name = dest.display_name();
            let dest_stats = Arc::clone(&self.dest_stats[idx]);

            task_handles.push(tokio::spawn(crate::stats::run_reporter(
                Arc::clone(&dest_stats),
                interval,
                self.shutdown_rx.clone(),
            )));

            all_stats.push(Arc::clone(&dest_stats));
            dest_channels.push(DestChannel {
                tx,
                policy: dest.overflow_policy,
                name: name.clone(),
                stats: Arc::clone(&dest_stats),
            });

            let cfg = dest.clone();
            let sd = self.shutdown_rx.clone();
            info!(dest = %name, index = idx, proto = ?cfg.protocol, mode = ?cfg.mode, "destination starting");
            task_handles.push(tokio::spawn(async move {
                if let Err(e) = run_destination(cfg, rx, dest_stats.clone(), sd, cap).await {
                    error!(dest = %name, error = %e, "destination failed");
                    dest_stats.add_error();
                }
            }));
        }

        if let Some(port) = self.config.general.health_port {
            task_handles.push(spawn_health_server(port, all_stats, self.shutdown_rx.clone()));
        }

        let channels: Arc<Vec<DestChannel>> = Arc::new(dest_channels);

        info!(source = %src_name, proto = ?self.config.source.protocol, mode = ?self.config.source.mode, "source starting");

        let source_fut = run_source(
            self.config.source.clone(),
            Arc::clone(&channels),
            Arc::clone(&source_stats),
            Arc::clone(&self.limiter),
            max_payload,
            self.shutdown_rx.clone(),
        );

        tokio::select! {
            res = source_fut => {
                if let Err(e) = res {
                    error!(error = %e, "source stopped with error");
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl+C, shutting down");
            }
        }

        // Signal all tasks to stop, then wait up to 5 s for them to drain cleanly.
        let _ = self.shutdown_tx.send(true);
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            async {
                for handle in task_handles {
                    let _ = handle.await;
                }
            },
        )
        .await;
        info!("relay stopped");
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════
//  Config hot-reload
// ════════════════════════════════════════════════════════════════════

fn spawn_config_watcher(
    config_path: String,
    current_cfg: Arc<RwLock<RelayConfig>>,
    limiter: SharedLimiter,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    use notify::{RecursiveMode, Watcher};

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

        let mut watcher = match notify::recommended_watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        let _ = tx.try_send(());
                    }
                }
            },
        ) {
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
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    while rx.try_recv().is_ok() {}

                    match crate::config::RelayConfig::from_file(&config_path) {
                        Ok(new_cfg) => {
                            let old = current_cfg
                                .read()
                                .unwrap_or_else(|p| p.into_inner())
                                .clone();
                            apply_hot_reload(&old, &new_cfg, &limiter);
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

fn spawn_health_server(
    port: u16,
    all_stats: Vec<Arc<Stats>>,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let addr: SocketAddr = ([0, 0, 0, 0], port).into();
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                warn!(port = port, error = %e, "health server failed to bind");
                return;
            }
        };
        info!(port = port, "health server listening — GET /health  GET /stats");
        let sem = Arc::new(tokio::sync::Semaphore::new(32));

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            match Arc::clone(&sem).try_acquire_owned() {
                                Ok(permit) => {
                                    let stats = all_stats.clone();
                                    tokio::spawn(async move {
                                        handle_health_request(stream, stats).await;
                                        drop(permit);
                                    });
                                }
                                Err(_) => warn!("health server: connection limit reached, dropping"),
                            }
                        }
                        Err(e) => warn!(error = %e, "health server accept error"),
                    }
                }
                _ = shutdown.changed() => return,
            }
        }
    })
}

async fn handle_health_request(stream: tokio::net::TcpStream, all_stats: Vec<Arc<Stats>>) {
    // Drop the connection if the client stalls during request or response.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        serve_health_request(stream, all_stats),
    )
    .await;
}

async fn serve_health_request(stream: tokio::net::TcpStream, all_stats: Vec<Arc<Stats>>) {
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

    let (status, body) = match path {
        "/health" => ("200 OK", r#"{"status":"ok"}"#.to_string()),
        "/stats" => {
            let snapshots: Vec<_> = all_stats.iter().map(|s| s.snapshot()).collect();
            match serde_json::to_string(&snapshots) {
                Ok(json) => ("200 OK", json),
                Err(_) => (
                    "500 Internal Server Error",
                    r#"{"error":"serialize failed"}"#.to_string(),
                ),
            }
        }
        _ => ("404 Not Found", r#"{"error":"not found"}"#.to_string()),
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = writer.write_all(response.as_bytes()).await;
}

/// Apply the fields that can be changed without restarting tasks.
///
/// Currently: rate_limit.  Warns only when source/destinations/general fields
/// actually differ from the running config, avoiding false positives on
/// rate-limit-only reloads.
fn apply_hot_reload(old: &RelayConfig, new: &RelayConfig, limiter: &SharedLimiter) {
    let new_limiter = new.rate_limit.as_ref().map(|rl| {
        Arc::new(RateLimiter::new(rl.bytes_per_second, rl.burst_size))
    });
    *limiter.write().unwrap_or_else(|p| p.into_inner()) = new_limiter;

    match &new.rate_limit {
        Some(rl) => info!(
            bytes_per_second = rl.bytes_per_second,
            burst            = rl.burst_size,
            "config reloaded: rate limit updated"
        ),
        None => info!("config reloaded: rate limiting disabled"),
    }

    if new.source != old.source
        || new.destinations != old.destinations
        || new.general != old.general
    {
        warn!("config reloaded: changes to source, destinations, or general settings require a restart");
    }
}

// ════════════════════════════════════════════════════════════════════
//  Source
// ════════════════════════════════════════════════════════════════════

async fn run_source(
    cfg: EndpointConfig,
    channels: Arc<Vec<DestChannel>>,
    stats: Arc<Stats>,
    limiter: SharedLimiter,
    max_payload: usize,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    match (cfg.protocol, cfg.mode) {
        (Protocol::Tcp, EndpointMode::Server) => {
            source_tcp_server(cfg, channels, stats, limiter, max_payload, shutdown).await
        }
        (Protocol::Tcp, EndpointMode::Client) => {
            source_tcp_client(cfg, channels, stats, limiter, max_payload, shutdown).await
        }
        (Protocol::Udp, EndpointMode::Server | EndpointMode::Client) => {
            source_udp(cfg, channels, stats, limiter, max_payload, shutdown).await
        }
    }
}

// ── TCP source: server ─────────────────────────────────────────────

async fn source_tcp_server(
    cfg: EndpointConfig,
    channels: Arc<Vec<DestChannel>>,
    stats: Arc<Stats>,
    limiter: SharedLimiter,
    max_payload: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let listener = tokio::task::spawn_blocking(move || transport::bind_tcp_listener(addr))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??;
    info!(address = %addr, "TCP source listening");

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                info!(peer = %peer, "source: TCP connection accepted");
                stats.conn_open();

                let channels = Arc::clone(&channels);
                let stats = Arc::clone(&stats);
                let limiter = Arc::clone(&limiter);
                let mut sd = shutdown.clone();

                tokio::spawn(async move {
                    if let Err(e) = relay_tcp_reader(
                        stream, &channels, &stats, &limiter, max_payload, &mut sd,
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
    channels: Arc<Vec<DestChannel>>,
    stats: Arc<Stats>,
    limiter: SharedLimiter,
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
                let res = relay_tcp_reader(
                    stream, &channels, &stats, &limiter, max_payload, &mut sd,
                ).await;
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
    channels: &[DestChannel],
    stats: &Stats,
    limiter: &SharedLimiter,
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
                // Clone the Arc out of the lock before awaiting so we never
                // hold the RwLockReadGuard across an await point.
                let current = limiter.read().unwrap_or_else(|p| p.into_inner()).clone();
                if let Some(ref rl) = current {
                    rl.acquire(n as u64).await;
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
    channels: Arc<Vec<DestChannel>>,
    stats: Arc<Stats>,
    limiter: SharedLimiter,
    max_payload: usize,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let socket = if cfg.mode == EndpointMode::Server {
        let cfg_for_bind = cfg.clone();
        tokio::task::spawn_blocking(move || transport::bind_udp_recv(addr, &cfg_for_bind))
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??
    } else {
        let cfg_for_bind = cfg.clone();
        let s = tokio::task::spawn_blocking(move || transport::bind_udp_send(addr, &cfg_for_bind))
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??;
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
                        stats_recv.add_error();
                        continue;
                    }
                    stats_recv.add_received(n as u64);
                    debug!(bytes = n, peer = %peer, "UDP source recv");
                    let data = Bytes::copy_from_slice(&buf[..n]);
                    if recv_tx.try_send(data).is_err() {
                        stats_recv.add_dropped(1);
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
                    Some(data) => {
                        let current = limiter.read().unwrap_or_else(|p| p.into_inner()).clone();
                        if let Some(ref rl) = current {
                            rl.acquire(data.len() as u64).await;
                        }
                        send_to_all(&channels, data).await;
                    }
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
    rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    shutdown: watch::Receiver<bool>,
    channel_cap: usize,
) -> Result<()> {
    match (cfg.protocol, cfg.mode) {
        (Protocol::Tcp, EndpointMode::Client) => {
            dest_tcp_client(cfg, rx, stats, shutdown).await
        }
        (Protocol::Tcp, EndpointMode::Server) => {
            dest_tcp_server(cfg, rx, stats, shutdown, channel_cap).await
        }
        (Protocol::Udp, EndpointMode::Client) => {
            dest_udp_client(cfg, rx, stats, shutdown).await
        }
        (Protocol::Udp, EndpointMode::Server) => {
            dest_udp_server(cfg, rx, stats, shutdown).await
        }
    }
}

// ── TCP destination: client (connect-out with reconnect) ───────────

async fn dest_tcp_client(
    cfg: DestConfig,
    mut rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let delay = cfg.reconnect_delay();
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
                    }
                    None => return Ok(()),
                }
            }
            Err(e) => {
                warn!(dest = %name, error = %e, "dest: TCP connect failed");
                stats.add_error();
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

// ── TCP destination: server (listen, fan-out to connected peers) ───

async fn dest_tcp_server(
    cfg: DestConfig,
    rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
    channel_cap: usize,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();
    let listener = tokio::task::spawn_blocking(move || transport::bind_tcp_listener(addr))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??;
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
                                    stats.add_dropped(1);
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
    loop {
        match rx.recv().await {
            Some(data) => {
                if let Err(e) = writer.write_all(&data).await {
                    debug!(dest = %dest_name, peer = %peer, error = %e, "dest-server: write failed");
                    stats.add_error();
                    break;
                }
                stats.add_sent(data.len() as u64);
            }
            None => break, // sender dropped: either peer replaced or dest shutting down
        }
    }
    stats.conn_close();
}

// ── UDP destination: client (send-to) ──────────────────────────────

async fn dest_udp_client(
    cfg: DestConfig,
    mut rx: mpsc::Receiver<Bytes>,
    stats: Arc<Stats>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let addr = cfg.socket_addr()?;
    let name = cfg.display_name();
    let cfg_for_bind = cfg.clone();
    let socket = tokio::task::spawn_blocking(move || transport::bind_udp_send(addr, &cfg_for_bind))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??;
    info!(dest = %name, address = %addr, cast = ?cfg.cast_mode, "dest: UDP client ready");

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(data) => {
                        match socket.send_to(&data, addr).await {
                            Ok(n) => {
                                stats.add_sent(n as u64);
                                stats.msg_relayed();
                            }
                            Err(e) => {
                                warn!(dest = %name, error = %e, "dest: UDP send error");
                                stats.add_error();
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
        assert!(relay.source_stats.label.starts_with("source("), "label: {}", relay.source_stats.label);
        assert_eq!(relay.dest_stats.len(), 1);
        assert!(relay.dest_stats[0].label.starts_with("dest("), "label: {}", relay.dest_stats[0].label);
    }

    #[test]
    fn relay_new_stats_count_matches_destinations() {
        let mut cfg = test_config();
        cfg.destinations.push(cfg.destinations[0].clone());
        let relay = Relay::new(cfg, "/tmp/test.toml".to_string());
        assert_eq!(relay.dest_stats.len(), 2);
    }

    #[tokio::test]
    async fn send_to_all_drop_newest_increments_dropped() {
        let (tx, mut rx) = mpsc::channel::<Bytes>(1);
        tx.try_send(Bytes::from("fill")).unwrap(); // fill to capacity
        let stats = Arc::new(Stats::new("dest", "", ""));
        let ch = DestChannel {
            tx,
            policy: OverflowPolicy::DropNewest,
            name: "dest".to_string(),
            stats: Arc::clone(&stats),
        };
        send_to_all(&[ch], Bytes::from("overflow")).await;
        assert_eq!(stats.dropped_messages.load(Ordering::Relaxed), 1);
        assert!(rx.try_recv().is_ok()); // original fill item still there
    }

    #[tokio::test]
    async fn send_to_all_block_delivers_when_space_available() {
        let (tx, mut rx) = mpsc::channel::<Bytes>(4);
        let stats = Arc::new(Stats::new("dest", "", ""));
        let ch = DestChannel {
            tx,
            policy: OverflowPolicy::Block,
            name: "dest".to_string(),
            stats: Arc::clone(&stats),
        };
        let data = Bytes::from("hello");
        send_to_all(&[ch], data.clone()).await;
        let received = rx.recv().await.unwrap();
        assert_eq!(received, data);
        assert_eq!(stats.dropped_messages.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn apply_hot_reload_installs_rate_limiter() {
        let old = test_config();
        let mut new = test_config();
        new.rate_limit = Some(RateLimitConfig { bytes_per_second: 500, burst_size: 1000 });
        let limiter: SharedLimiter = Arc::new(RwLock::new(None));
        apply_hot_reload(&old, &new, &limiter);
        assert!(limiter.read().unwrap().is_some());
    }

    #[test]
    fn apply_hot_reload_removes_rate_limiter() {
        let mut old = test_config();
        old.rate_limit = Some(RateLimitConfig { bytes_per_second: 1000, burst_size: 5000 });
        let new = test_config(); // no rate_limit
        let limiter: SharedLimiter = Arc::new(RwLock::new(
            Some(Arc::new(RateLimiter::new(1000, 5000))),
        ));
        apply_hot_reload(&old, &new, &limiter);
        assert!(limiter.read().unwrap().is_none());
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
                serve_health_request(stream, stats).await;
            })
        };

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
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
                serve_health_request(stream, stats).await;
            })
        };

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /stats HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("200 OK"), "got: {response}");
        assert!(response.contains("rx_bytes"), "got: {response}");
        assert!(response.contains("\"label\":\"src\""), "got: {response}");
    }

    #[tokio::test]
    async fn unknown_path_returns_404() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stats: Vec<Arc<Stats>> = vec![];

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_health_request(stream, stats).await;
        });

        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /unknown HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();

        assert!(response.contains("404"), "got: {response}");
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
            let channels = vec![
                DestChannel { tx: tx1, policy: OverflowPolicy::Block, name: "d1".into(), stats: Arc::clone(&stats1) },
                DestChannel { tx: tx2, policy: OverflowPolicy::Block, name: "d2".into(), stats: Arc::clone(&stats2) },
            ];
            let payload = Bytes::from("broadcast-payload");
            send_to_all(&channels, payload.clone()).await;
            assert_eq!(rx1.recv().await.unwrap(), payload);
            assert_eq!(rx2.recv().await.unwrap(), payload);
        });
    }

    #[test]
    fn apply_hot_reload_same_config_no_panic() {
        let cfg = test_config();
        let limiter: SharedLimiter = Arc::new(RwLock::new(None));
        apply_hot_reload(&cfg, &cfg, &limiter);
        assert!(limiter.read().unwrap().is_none());
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
            serve_health_request(stream, stats).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        server.await.unwrap();
        assert!(response.contains("Content-Length:"), "missing header:\n{response}");
        assert!(response.contains("Content-Type: application/json"), "missing content-type:\n{response}");
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
            serve_health_request(stream, stats).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /stats HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
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
            serve_health_request(stream, stats).await;
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET /health?v=1 HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
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
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))??
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
