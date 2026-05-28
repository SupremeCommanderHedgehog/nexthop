// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Scenario 5 of #24: hot-reload of `[rate_limit]` actually re-budgets
//! the destination egress on the next packet.
//!
//! Two phases share the same relay process:
//!
//! 1. Start with a very tight per-destination rate limit
//!    (small burst, low rate). Send a burst, time how long the dest
//!    socket needs to drain it.
//! 2. Rewrite the config to remove the rate limit entirely. Wait for
//!    the notify watcher to pick up the change (~200 ms debounce,
//!    plus slack). Send the same burst again and time the drain.
//!
//! Under a correct hot-reload, phase 2's drain is substantially
//! faster than phase 1's. If hot-reload were broken (the old limiter
//! still applied), phase 2 would take just as long as phase 1.
//!
//! The assertion is a soft ratio (phase 1 ≥ 1.5 × phase 2) rather
//! than an absolute timing bound. Loopback UDP delivery is fast in
//! both phases; the comparison is what matters.

mod common;

use common::*;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

const N: u32 = 30;
/// 500 bytes / 30 packets = 15 KiB total. With phase-1 rate of 8000
/// bytes/sec, that's about ~2 seconds of throttled drain — long
/// enough to measure reliably, short enough to keep the test under
/// 6 seconds wall.
const PAYLOAD_LEN: usize = 500;

#[test]
fn hot_reload_of_rate_limit_changes_throughput() {
    let source_port = ephemeral_udp_port();
    let dest_port = ephemeral_udp_port();
    let dest_sock = UdpSocket::bind(format!("127.0.0.1:{dest_port}")).expect("bind dest");
    dest_sock
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let config_path = temp_config_path("rate-reload");

    // Phase 1: tight per-destination rate limit. 8 KB/sec sustained,
    // 1 KB burst. The first ~1 KB of payload flows through quickly;
    // the rest is metered at the sustained rate.
    write_config_with_dest_rate(&config_path, source_port, dest_port, Some((8_000, 1_000)));

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };
    std::thread::sleep(STARTUP_DELAY);

    let phase1 = run_burst_and_time(&dest_sock, source_port);

    // Phase 2: drop the per-destination rate limit. Wait for the
    // watcher to debounce + reload. We deliberately overshoot the
    // debounce (200 ms) to leave room for FS event jitter.
    write_config_with_dest_rate(&config_path, source_port, dest_port, None);
    std::thread::sleep(HOT_RELOAD_DELAY);

    let phase2 = run_burst_and_time(&dest_sock, source_port);

    println!("phase1 (rate-limited): {phase1:?}");
    println!("phase2 (uncapped):     {phase2:?}");

    assert!(
        phase1 >= phase2 * 3 / 2,
        "expected the rate-limited phase 1 ({phase1:?}) to be substantially \
         slower than the uncapped phase 2 ({phase2:?}); hot-reload may not be \
         applying"
    );

    drop(guard);
}

fn write_config_with_dest_rate(
    path: &std::path::Path,
    source_port: u16,
    dest_port: u16,
    rate: Option<(u64, u64)>,
) {
    let mut dest = DestSpec::from_endpoint(EndpointSpec::udp_client(dest_port));
    if let Some((bps, burst)) = rate {
        dest = dest.with_rate_limit(bps, burst);
    }
    write_config(
        path,
        &ConfigSpec::new(EndpointSpec::udp_server(source_port), vec![dest]),
    );
}

fn run_burst_and_time(dest_sock: &UdpSocket, source_port: u16) -> Duration {
    // Drain any stragglers from a previous phase before we start.
    // We can't use the full 5-second read timeout the socket carries
    // for the actual drain loop — predrain would block for 5 seconds
    // whenever the buffer is already empty, which is the common case.
    dest_sock
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    while udp_try_recv(dest_sock, "predrain").is_some() {}
    dest_sock
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let tx = UdpSocket::bind("127.0.0.1:0").expect("bind tx");
    let source_addr = format!("127.0.0.1:{source_port}");
    let payload = vec![0x42u8; PAYLOAD_LEN];

    let started = Instant::now();
    for _ in 0..N {
        tx.send_to(&payload, &source_addr).expect("send");
        // 1-ms inter-send pace so the source's UDP recv_tx (capacity
        // 64) never overruns — the rate limiter has to be the
        // bottleneck under test, not the ingress.
        std::thread::sleep(Duration::from_millis(1));
    }

    // Drain until we've received all N (or hit a stall longer than
    // the per-recv read timeout).
    let mut received = 0usize;
    while received < N as usize {
        match udp_try_recv(dest_sock, "drain") {
            Some(_) => received += 1,
            None => break, // read_timeout fired
        }
    }
    let elapsed = started.elapsed();
    assert!(
        received >= (N as usize) * 9 / 10,
        "drain saw only {received}/{N} packets within the read window; \
         neither phase should drop on loopback at this scale"
    );
    elapsed
}
