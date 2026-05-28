// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon
//
// nexthop - TCP / UDP - Unicast / Multicast / Broadcast
// Architect: Patrick S Connallon
// Built by:  Anthropic Claude (Sonnet 4.6)

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::watch;
use tokio::time::{Duration, Instant};
use tracing::info;

/// Point-in-time snapshot of one endpoint's counters; serializable to JSON.
///
/// The `dropped` field is the sum of the four `dropped_*` sub-counters and is
/// kept for backwards compatibility with consumers of the pre-breakdown /stats
/// response. New code should prefer the per-reason fields.
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
    pub dropped_overflow: u64,
    pub dropped_oversize: u64,
    pub dropped_validation: u64,
    pub dropped_write_error: u64,
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
    /// Sum of `dropped_*` sub-counters. Kept as a denormalized cache so
    /// snapshot() reads it directly without a multi-load summation.
    pub dropped_messages: AtomicU64,
    pub dropped_overflow: AtomicU64,
    pub dropped_oversize: AtomicU64,
    pub dropped_validation: AtomicU64,
    pub dropped_write_error: AtomicU64,
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
            dropped_overflow: AtomicU64::new(0),
            dropped_oversize: AtomicU64::new(0),
            dropped_validation: AtomicU64::new(0),
            dropped_write_error: AtomicU64::new(0),
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

    /// Queue-full drops (destination's mpsc channel is full + drop_newest
    /// policy, or a UDP server's per-peer channel is full).
    pub fn add_dropped_overflow(&self, n: u64) {
        self.dropped_overflow.fetch_add(n, Ordering::Relaxed);
        self.dropped_messages.fetch_add(n, Ordering::Relaxed);
    }

    /// Datagrams exceeding `general.max_payload_size`.
    pub fn add_dropped_oversize(&self, n: u64) {
        self.dropped_oversize.fetch_add(n, Ordering::Relaxed);
        self.dropped_messages.fetch_add(n, Ordering::Relaxed);
    }

    /// Validation failures. Reserved for future content/header checks —
    /// the `dropped_validation` counter is exposed in /stats and /metrics
    /// today but no code path increments it yet.
    #[allow(dead_code)]
    pub fn add_dropped_validation(&self, n: u64) {
        self.dropped_validation.fetch_add(n, Ordering::Relaxed);
        self.dropped_messages.fetch_add(n, Ordering::Relaxed);
    }

    /// Writes to a destination that failed before the packet was delivered.
    pub fn add_dropped_write_error(&self, n: u64) {
        self.dropped_write_error.fetch_add(n, Ordering::Relaxed);
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
            dropped_overflow: self.dropped_overflow.load(Ordering::Relaxed),
            dropped_oversize: self.dropped_oversize.load(Ordering::Relaxed),
            dropped_validation: self.dropped_validation.load(Ordering::Relaxed),
            dropped_write_error: self.dropped_write_error.load(Ordering::Relaxed),
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

/// Render a set of snapshots as Prometheus text-exposition format
/// (`text/plain; version=0.0.4`).
///
/// Counter naming follows Prometheus conventions: `_total` suffix on
/// monotonic counters, base units (`_bytes`, `_seconds`) in the metric
/// name. Endpoint names are passed through `escape_label` so arbitrary
/// user-provided strings can't break the parser.
pub fn render_prometheus(snapshots: &[StatsSnapshot]) -> String {
    fn block_counter(out: &mut String, name: &str, help: &str) {
        out.push_str(&format!("# HELP {name} {help}\n"));
        out.push_str(&format!("# TYPE {name} counter\n"));
    }
    fn block_gauge(out: &mut String, name: &str, help: &str) {
        out.push_str(&format!("# HELP {name} {help}\n"));
        out.push_str(&format!("# TYPE {name} gauge\n"));
    }
    fn row(out: &mut String, name: &str, label: &str, value: u64) {
        out.push_str(&format!("{name}{{endpoint={label}}} {value}\n"));
    }

    let mut out = String::with_capacity(snapshots.len() * 512);

    // ── Counters ──
    block_counter(
        &mut out,
        "nexthop_rx_bytes_total",
        "Total bytes received per endpoint since process start.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_rx_bytes_total",
            &escape_label(&s.label),
            s.rx_bytes,
        );
    }

    block_counter(
        &mut out,
        "nexthop_tx_bytes_total",
        "Total bytes transmitted per endpoint since process start.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_tx_bytes_total",
            &escape_label(&s.label),
            s.tx_bytes,
        );
    }

    block_counter(
        &mut out,
        "nexthop_messages_total",
        "Total messages relayed per endpoint since process start.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_messages_total",
            &escape_label(&s.label),
            s.messages,
        );
    }

    block_counter(
        &mut out,
        "nexthop_errors_total",
        "Total endpoint errors since process start.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_errors_total",
            &escape_label(&s.label),
            s.errors,
        );
    }

    block_counter(
        &mut out,
        "nexthop_dropped_total",
        "Total packets dropped per endpoint since process start (sum of dropped_* by reason).",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_dropped_total",
            &escape_label(&s.label),
            s.dropped,
        );
    }

    // Per-reason breakdown of dropped packets. Sums to nexthop_dropped_total.
    block_counter(
        &mut out,
        "nexthop_dropped_overflow_total",
        "Packets dropped because the destination queue was full (drop_newest policy).",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_dropped_overflow_total",
            &escape_label(&s.label),
            s.dropped_overflow,
        );
    }

    block_counter(
        &mut out,
        "nexthop_dropped_oversize_total",
        "Packets dropped because they exceeded general.max_payload_size.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_dropped_oversize_total",
            &escape_label(&s.label),
            s.dropped_oversize,
        );
    }

    block_counter(
        &mut out,
        "nexthop_dropped_validation_total",
        "Packets dropped due to validation failure (reserved).",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_dropped_validation_total",
            &escape_label(&s.label),
            s.dropped_validation,
        );
    }

    block_counter(
        &mut out,
        "nexthop_dropped_write_error_total",
        "Packets dropped because writing to the destination failed.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_dropped_write_error_total",
            &escape_label(&s.label),
            s.dropped_write_error,
        );
    }

    block_counter(
        &mut out,
        "nexthop_connections_opened_total",
        "Total connections opened against this endpoint since process start.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_connections_opened_total",
            &escape_label(&s.label),
            s.total_connections,
        );
    }

    // ── Gauges ──
    block_gauge(
        &mut out,
        "nexthop_active_connections",
        "Currently open connections to this endpoint.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_active_connections",
            &escape_label(&s.label),
            s.active_connections,
        );
    }

    block_gauge(
        &mut out,
        "nexthop_uptime_seconds",
        "Seconds since the endpoint's task started.",
    );
    for s in snapshots {
        row(
            &mut out,
            "nexthop_uptime_seconds",
            &escape_label(&s.label),
            s.uptime_s,
        );
    }

    out
}

/// Quote and escape a label value for Prometheus text-exposition format.
/// Spec: backslash, double-quote, and newlines must be escaped; everything
/// else passes through.
fn escape_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Periodically logs a statistics summary until shutdown.
///
/// The interval is read from the shared atomic on every tick so a
/// hot-reload of `general.stats_interval_secs` takes effect on the next
/// cycle without restarting the reporter task.
pub async fn run_reporter(
    stats: Arc<Stats>,
    interval_secs: Arc<AtomicU64>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        // Guard against 0 producing a zero-duration busy loop.
        let secs = interval_secs.load(Ordering::Relaxed).max(1);
        let sleep = tokio::time::sleep(Duration::from_secs(secs));
        tokio::select! {
            _ = sleep => stats.log_report(),
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
        s.add_dropped_overflow(3);
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
        s.add_dropped_overflow(5);
        s.add_dropped_overflow(3);
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
        s.add_dropped_overflow(7);
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
        assert!(
            json.contains("\"peer_addr\":\"remote:2000\""),
            "got: {json}"
        );
    }

    // ── Per-reason dropped breakdown ───────────────────────────────────

    #[test]
    fn add_dropped_overflow_increments_sub_and_total() {
        let s = Stats::new("t", "", "");
        s.add_dropped_overflow(3);
        assert_eq!(s.dropped_overflow.load(Ordering::Relaxed), 3);
        assert_eq!(s.dropped_messages.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn add_dropped_oversize_increments_sub_and_total() {
        let s = Stats::new("t", "", "");
        s.add_dropped_oversize(2);
        assert_eq!(s.dropped_oversize.load(Ordering::Relaxed), 2);
        assert_eq!(s.dropped_messages.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn add_dropped_validation_increments_sub_and_total() {
        let s = Stats::new("t", "", "");
        s.add_dropped_validation(4);
        assert_eq!(s.dropped_validation.load(Ordering::Relaxed), 4);
        assert_eq!(s.dropped_messages.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn add_dropped_write_error_increments_sub_and_total() {
        let s = Stats::new("t", "", "");
        s.add_dropped_write_error(5);
        assert_eq!(s.dropped_write_error.load(Ordering::Relaxed), 5);
        assert_eq!(s.dropped_messages.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn snapshot_dropped_equals_sum_of_breakdown() {
        let s = Stats::new("t", "", "");
        s.add_dropped_overflow(1);
        s.add_dropped_oversize(2);
        s.add_dropped_validation(3);
        s.add_dropped_write_error(4);
        let snap = s.snapshot();
        assert_eq!(snap.dropped_overflow, 1);
        assert_eq!(snap.dropped_oversize, 2);
        assert_eq!(snap.dropped_validation, 3);
        assert_eq!(snap.dropped_write_error, 4);
        assert_eq!(snap.dropped, 10, "dropped should equal sum of sub-counters");
    }

    #[test]
    fn prometheus_includes_per_reason_dropped_metrics() {
        let s = Stats::new("dest(b)", "", "");
        s.add_dropped_overflow(5);
        s.add_dropped_oversize(1);
        s.add_dropped_write_error(2);
        let body = render_prometheus(&[s.snapshot()]);

        for (name, val) in [
            ("nexthop_dropped_overflow_total", 5),
            ("nexthop_dropped_oversize_total", 1),
            ("nexthop_dropped_validation_total", 0),
            ("nexthop_dropped_write_error_total", 2),
        ] {
            let expected = format!("{name}{{endpoint=\"dest(b)\"}} {val}");
            assert!(
                body.contains(&expected),
                "missing {expected}; body=\n{body}"
            );
        }
        // Sum metric still present.
        assert!(
            body.contains("nexthop_dropped_total{endpoint=\"dest(b)\"} 8"),
            "body=\n{body}"
        );
    }

    // ── render_prometheus / escape_label ───────────────────────────────

    fn snap_with(label: &str, rx: u64, tx: u64, dropped: u64, active: u64) -> StatsSnapshot {
        let s = Stats::new(label, "", "");
        s.add_received(rx);
        s.add_sent(tx);
        s.add_dropped_overflow(dropped);
        for _ in 0..active {
            s.conn_open();
        }
        s.snapshot()
    }

    #[test]
    fn prometheus_emits_help_and_type_for_each_metric() {
        let body = render_prometheus(&[snap_with("source(ingest)", 100, 0, 0, 1)]);
        for name in [
            "nexthop_rx_bytes_total",
            "nexthop_tx_bytes_total",
            "nexthop_messages_total",
            "nexthop_errors_total",
            "nexthop_dropped_total",
            "nexthop_connections_opened_total",
            "nexthop_active_connections",
            "nexthop_uptime_seconds",
        ] {
            assert!(
                body.contains(&format!("# HELP {name} ")),
                "missing HELP for {name}; body=\n{body}"
            );
            assert!(
                body.contains(&format!("# TYPE {name} ")),
                "missing TYPE for {name}; body=\n{body}"
            );
        }
    }

    #[test]
    fn prometheus_counter_values_match_snapshot() {
        let body = render_prometheus(&[snap_with("dest(tcp-backend)", 0, 4096000, 3, 1)]);
        assert!(
            body.contains("nexthop_tx_bytes_total{endpoint=\"dest(tcp-backend)\"} 4096000"),
            "body=\n{body}"
        );
        assert!(
            body.contains("nexthop_dropped_total{endpoint=\"dest(tcp-backend)\"} 3"),
            "body=\n{body}"
        );
        assert!(
            body.contains("nexthop_active_connections{endpoint=\"dest(tcp-backend)\"} 1"),
            "body=\n{body}"
        );
    }

    #[test]
    fn prometheus_emits_one_row_per_endpoint_per_metric() {
        let body = render_prometheus(&[
            snap_with("source(a)", 0, 0, 0, 0),
            snap_with("dest(b)", 0, 0, 0, 0),
            snap_with("dest(c)", 0, 0, 0, 0),
        ]);
        // 3 endpoints, each should appear in nexthop_rx_bytes_total
        let rx_rows = body
            .lines()
            .filter(|l| l.starts_with("nexthop_rx_bytes_total{"))
            .count();
        assert_eq!(rx_rows, 3, "body=\n{body}");
    }

    #[test]
    fn prometheus_escapes_label_quotes_and_backslashes() {
        // A user puts weird characters in their endpoint name. The output must
        // remain parseable Prometheus text.
        let snap = snap_with(r#"weird"name\with\backslashes"#, 1, 0, 0, 0);
        let body = render_prometheus(&[snap]);
        // Quote and backslash get escaped; newlines stay literal in the name
        // (none here, but the escape is in place for them too).
        assert!(
            body.contains(r#"endpoint="weird\"name\\with\\backslashes""#),
            "body=\n{body}"
        );
    }

    #[test]
    fn prometheus_empty_snapshots_emits_only_help_and_type() {
        let body = render_prometheus(&[]);
        // Should still contain HELP/TYPE markers — Prometheus parsers accept
        // metric families with zero series.
        assert!(
            body.contains("# HELP nexthop_rx_bytes_total"),
            "body=\n{body}"
        );
        // But no value rows.
        assert!(
            !body.contains("nexthop_rx_bytes_total{"),
            "should have no rows; body=\n{body}"
        );
    }
}
