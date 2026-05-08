// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present nexthop@krypte.me
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: nexthop@krypte.me
// Built by:  Anthropic Claude (Sonnet 4.6)

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::watch;
use tokio::time::{Duration, Instant};
use tracing::info;

/// Point-in-time snapshot of one endpoint's counters; serializable to JSON.
#[derive(Serialize, Deserialize)]
pub struct StatsSnapshot {
    pub label: String,
    pub local_addr: String,
    pub peer_addr: String,
    pub snapshot_at: u64,
    pub uptime_s: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub messages: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub errors: u64,
    pub dropped: u64,
}

/// Shared, lock-free statistics counters for one named endpoint.
pub struct Stats {
    pub label: String,
    pub local_addr: String,
    pub peer_addr: String,
    pub bytes_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub messages_relayed: AtomicU64,
    pub active_connections: AtomicU64,
    pub total_connections: AtomicU64,
    pub errors: AtomicU64,
    pub dropped_messages: AtomicU64,
    start_time: Instant,
}

impl Stats {
    pub fn new(
        label: impl Into<String>,
        local_addr: impl Into<String>,
        peer_addr: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            local_addr: local_addr.into(),
            peer_addr: peer_addr.into(),
            bytes_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            messages_relayed: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            total_connections: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            dropped_messages: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    pub fn add_received(&self, n: u64) {
        self.bytes_received.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_sent(&self, n: u64) {
        self.bytes_sent.fetch_add(n, Ordering::Relaxed);
    }

    pub fn msg_relayed(&self) {
        self.messages_relayed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_dropped(&self, n: u64) {
        self.dropped_messages.fetch_add(n, Ordering::Relaxed);
    }

    pub fn conn_open(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn conn_close(&self) {
        self.active_connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            })
            .ok();
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let snapshot_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        StatsSnapshot {
            label: self.label.clone(),
            local_addr: self.local_addr.clone(),
            peer_addr: self.peer_addr.clone(),
            snapshot_at,
            uptime_s: self.start_time.elapsed().as_secs(),
            rx_bytes: self.bytes_received.load(Ordering::Relaxed),
            tx_bytes: self.bytes_sent.load(Ordering::Relaxed),
            messages: self.messages_relayed.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_connections: self.total_connections.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            dropped: self.dropped_messages.load(Ordering::Relaxed),
        }
    }

    pub fn log_report(&self) {
        let secs = self.start_time.elapsed().as_secs();
        info!(
            endpoint      = %self.label,
            uptime_s      = secs,
            rx_bytes      = self.bytes_received.load(Ordering::Relaxed),
            tx_bytes      = self.bytes_sent.load(Ordering::Relaxed),
            messages      = self.messages_relayed.load(Ordering::Relaxed),
            active_conns  = self.active_connections.load(Ordering::Relaxed),
            total_conns   = self.total_connections.load(Ordering::Relaxed),
            errors        = self.errors.load(Ordering::Relaxed),
            dropped       = self.dropped_messages.load(Ordering::Relaxed),
            "statistics"
        );
    }
}

/// Periodically logs a statistics summary until shutdown.
pub async fn run_reporter(stats: Arc<Stats>, interval: Duration, mut shutdown: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(interval);
    tick.tick().await; // consume the immediate first tick
    loop {
        tokio::select! {
            _ = tick.tick() => stats.log_report(),
            _ = shutdown.changed() => {
                stats.log_report();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_atomic() {
        let s = Stats::new("test", "", "");
        s.add_received(100);
        s.add_received(200);
        assert_eq!(s.bytes_received.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn conn_open_close() {
        let s = Stats::new("test", "", "");
        s.conn_open();
        s.conn_open();
        assert_eq!(s.active_connections.load(Ordering::Relaxed), 2);
        assert_eq!(s.total_connections.load(Ordering::Relaxed), 2);
        s.conn_close();
        assert_eq!(s.active_connections.load(Ordering::Relaxed), 1);
        assert_eq!(s.total_connections.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn conn_close_saturates_at_zero() {
        let s = Stats::new("test", "", "");
        s.conn_open();
        s.conn_close();
        s.conn_close(); // extra close must not wrap
        assert_eq!(s.active_connections.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn snapshot_label_matches() {
        let s = Stats::new("my-endpoint", "", "");
        assert_eq!(s.snapshot().label, "my-endpoint");
    }

    #[test]
    fn snapshot_captures_all_counters() {
        let s = Stats::new("test", "", "");
        s.add_received(100);
        s.add_sent(200);
        s.msg_relayed();
        s.msg_relayed();
        s.add_error();
        s.add_dropped(3);
        s.conn_open();
        let snap = s.snapshot();
        assert_eq!(snap.rx_bytes, 100);
        assert_eq!(snap.tx_bytes, 200);
        assert_eq!(snap.messages, 2);
        assert_eq!(snap.errors, 1);
        assert_eq!(snap.dropped, 3);
        assert_eq!(snap.active_connections, 1);
        assert_eq!(snap.total_connections, 1);
    }

    #[test]
    fn snapshot_at_is_nonzero() {
        let s = Stats::new("test", "", "");
        assert!(s.snapshot().snapshot_at > 0);
    }

    #[test]
    fn snapshot_serializes_to_json() {
        let s = Stats::new("src", "", "");
        s.add_received(42);
        let json = serde_json::to_string(&s.snapshot()).expect("serialize");
        assert!(json.contains("\"rx_bytes\":42"), "got: {json}");
        assert!(json.contains("\"label\":\"src\""), "got: {json}");
    }

    #[test]
    fn active_never_exceeds_total() {
        let s = Stats::new("test", "", "");
        for _ in 0..5 {
            s.conn_open();
        }
        for _ in 0..3 {
            s.conn_close();
        }
        let snap = s.snapshot();
        assert_eq!(snap.active_connections, 2);
        assert_eq!(snap.total_connections, 5);
        assert!(snap.active_connections <= snap.total_connections);
    }

    // ── Additional coverage ────────────────────────────────────────────

    #[test]
    fn snapshot_includes_local_and_peer_addr() {
        let s = Stats::new("ep", "0.0.0.0:5000", "192.168.1.10:4567");
        let snap = s.snapshot();
        assert_eq!(snap.local_addr, "0.0.0.0:5000");
        assert_eq!(snap.peer_addr, "192.168.1.10:4567");
    }

    #[test]
    fn add_sent_accumulates() {
        let s = Stats::new("t", "", "");
        s.add_sent(100);
        s.add_sent(50);
        assert_eq!(s.bytes_sent.load(Ordering::Relaxed), 150);
    }

    #[test]
    fn msg_relayed_increments() {
        let s = Stats::new("t", "", "");
        s.msg_relayed();
        s.msg_relayed();
        s.msg_relayed();
        assert_eq!(s.messages_relayed.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn add_dropped_accumulates() {
        let s = Stats::new("t", "", "");
        s.add_dropped(5);
        s.add_dropped(3);
        assert_eq!(s.dropped_messages.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn add_error_accumulates() {
        let s = Stats::new("t", "", "");
        s.add_error();
        s.add_error();
        assert_eq!(s.errors.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn snapshot_tx_bytes_matches_add_sent() {
        let s = Stats::new("t", "", "");
        s.add_sent(999);
        assert_eq!(s.snapshot().tx_bytes, 999);
    }

    #[test]
    fn snapshot_dropped_matches_add_dropped() {
        let s = Stats::new("t", "", "");
        s.add_dropped(7);
        assert_eq!(s.snapshot().dropped, 7);
    }

    #[test]
    fn log_report_does_not_panic() {
        let s = Stats::new("test", "0.0.0.0:1234", "(any)");
        s.add_received(100);
        s.add_sent(50);
        s.msg_relayed();
        s.conn_open();
        s.log_report(); // should not panic
    }

    #[test]
    fn snapshot_uptime_is_nonnegative() {
        let s = Stats::new("t", "", "");
        assert_eq!(s.snapshot().uptime_s, 0); // created just now
    }

    #[test]
    fn snapshot_json_contains_addr_fields() {
        let s = Stats::new("src", "lo:1000", "remote:2000");
        let json = serde_json::to_string(&s.snapshot()).unwrap();
        assert!(json.contains("\"local_addr\":\"lo:1000\""), "got: {json}");
        assert!(json.contains("\"peer_addr\":\"remote:2000\""), "got: {json}");
    }
}
