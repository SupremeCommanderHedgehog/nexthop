// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Scenario 4 of #24: TCP source recovers when the upstream peer
//! disconnects.
//!
//! The relay can run as a TCP **client** on the source side, dialing
//! out to a producer. If the producer goes away, the relay must
//! reconnect on its own. Symmetric to scenario 3 but on the inbound
//! side.
//!
//! Verification: we bind a TCP listener that pretends to be the
//! upstream producer. The relay connects to us. We immediately drop
//! the stream. The relay's source loop sees the EOF, sleeps for
//! `reconnect_delay_ms`, and reconnects. We measure the gap between
//! the first and second `accept()` to confirm both the recovery
//! itself and that it respects the configured delay.

mod common;

use common::*;
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn tcp_source_client_reconnects_after_upstream_disconnects() {
    const RECONNECT_DELAY_MS: u64 = 300;

    // We are the upstream that the relay's source dials.
    let upstream = TcpListener::bind("127.0.0.1:0").expect("bind upstream");
    let upstream_port = upstream.local_addr().expect("local").port();

    let dest_port = ephemeral_udp_port();
    let _dest_sock =
        std::net::UdpSocket::bind(format!("127.0.0.1:{dest_port}")).expect("bind dest sink");

    let (tx, rx) = mpsc::channel::<Instant>();
    let upstream_thread = std::thread::spawn(move || {
        for _ in 0..3 {
            match upstream.accept() {
                Ok((stream, _)) => {
                    let t = Instant::now();
                    drop(stream);
                    if tx.send(t).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    let config_path = temp_config_path("source-reconnect");
    let mut cfg = ConfigSpec::new(
        EndpointSpec::tcp_client(upstream_port),
        vec![DestSpec::from_endpoint(EndpointSpec::udp_client(dest_port))],
    );
    write_config(&config_path, &cfg);
    inject_source_reconnect_delay(&config_path, RECONNECT_DELAY_MS);
    let _ = &mut cfg;

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    let first = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("relay never made its first source TCP connection");

    let second = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("relay did not reconnect to the source after upstream closed");

    let gap = second.duration_since(first);
    assert!(
        gap >= Duration::from_millis(RECONNECT_DELAY_MS / 2),
        "source reconnect happened too quickly ({gap:?}); expected at least \
         half of the {RECONNECT_DELAY_MS}-ms reconnect delay"
    );
    assert!(
        gap <= Duration::from_secs(2),
        "source reconnect took {gap:?}; far longer than the configured \
         {RECONNECT_DELAY_MS}-ms delay"
    );

    // Third reconnect — verifies the loop is durable, not one-shot.
    rx.recv_timeout(Duration::from_secs(2))
        .expect("relay source stopped reconnecting after the second cycle");

    drop(guard);
    let _ = upstream_thread.join();
}

/// Append `reconnect_delay_ms = N` to the `[source]` table.
fn inject_source_reconnect_delay(path: &std::path::Path, ms: u64) {
    let s = std::fs::read_to_string(path).expect("read config");
    let mut out = String::with_capacity(s.len() + 64);
    for line in s.lines() {
        out.push_str(line);
        out.push('\n');
        if line.trim_start().starts_with("[source]") {
            out.push_str(&format!("reconnect_delay_ms = {ms}\n"));
        }
    }
    std::fs::write(path, out).expect("rewrite config");
}
