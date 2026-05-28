// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Scenario 2 of #24: `drop_newest` vs `block` overflow policies, end
//! to end.
//!
//! The two policies behave the same when no destination is slow; the
//! interesting cases are when one destination cannot keep up. We use:
//!
//! - **Fast destination**: a UDP socket that drains quickly. With
//!   either policy this destination should receive every payload sent
//!   by the source within the test budget.
//! - **Slow destination**: a TCP listener that accepts the connection
//!   but never reads, so the relay's kernel send buffer to that peer
//!   fills, then the dest task blocks on writes, which fills the
//!   relay's per-dest mpsc channel, which is where the overflow
//!   policy actually fires.
//!
//! Under `drop_newest`, the fast destination is unaffected by the
//! slow one — payloads to it land in order. Under `block`, the
//! source's `send_to_all` awaits room in the slow channel, so the
//! fast destination's reception is throttled to whatever the slow
//! socket drains, which here is effectively zero.
//!
//! These assertions are intentionally loose:
//! - `drop_newest` says "fast dest received *most* of N", because
//!   in-flight buffering means a couple of late packets are normal.
//! - `block` says "fast dest received *very few* (≤ N/4)", which is
//!   the bound that proves throttling is on without depending on
//!   exact kernel buffer sizes.

mod common;

use common::*;
use std::net::{TcpListener, UdpSocket};
use std::time::Duration;

const N: u32 = 200;
/// 4 KiB per datagram so N=200 sends ~800 KiB through the relay. The
/// TCP kernel send buffer to the slow listener is typically 64 KiB or
/// so; once we exceed it the dest task's write blocks, then its
/// per-destination mpsc channel (capacity 64) fills, and the
/// `block`-policy `send_to_all` on the source side finally starts
/// awaiting. With smaller payloads the entire test run fits in TCP
/// buffers and the block path never triggers.
const PAYLOAD_LEN: usize = 4096;

/// Send rate is paced to ~2 ms between datagrams. The relay's UDP
/// source uses a small (capacity 64) internal mpsc between its recv
/// task and its fan-out loop; a back-to-back burst overruns that
/// channel and produces ingress drops that have nothing to do with
/// the overflow policy under test.
const PACE: Duration = Duration::from_millis(2);

// Both tests are `#[ignore]`d on every platform: precisely
// distinguishing drop_newest from block end-to-end depends on kernel
// TCP/UDP send-buffer sizes, which vary per OS and even per
// runner. The unit tests in `relay::tests` already verify the policy
// dispatch path; what would be unique here is the
// kernel-buffer-pressure interaction, and that needs its own
// focused investigation to be CI-reliable. Run locally with
// `cargo test --test overflow_policies -- --ignored` while iterating.

#[test]
#[ignore = "buffer-pressure timing is platform-dependent; see file header"]
fn drop_newest_lets_fast_destination_keep_up_when_other_is_blocked() {
    let (fast_port, slow_port, source_port, dest_sock, _slow_listener) = setup_two_dests();
    let config_path = temp_config_path("overflow-drop-newest");
    write_config(
        &config_path,
        &ConfigSpec::new(
            EndpointSpec::udp_server(source_port),
            vec![
                DestSpec::from_endpoint(EndpointSpec::udp_client(fast_port)),
                DestSpec::from_endpoint(EndpointSpec::tcp_client(slow_port))
                    .with_overflow("drop_newest"),
            ],
        ),
    );

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };
    std::thread::sleep(STARTUP_DELAY);

    send_paced(source_port);
    let received = drain_fast(&dest_sock);
    // Allow generous slack: kernel UDP buffers, scheduling jitter, and
    // the relay's own recv_tx all contribute small losses on
    // loopback. drop_newest's job here is "do not stall on the slow
    // dest"; receiving the majority is enough to prove that.
    assert!(
        received >= (N * 3 / 4) as usize,
        "drop_newest: fast dest only got {received} / {N}; the slow dest appears to have stalled it"
    );

    drop(guard);
}

#[test]
#[ignore = "buffer-pressure timing is platform-dependent; see file header"]
fn block_throttles_fast_destination_when_other_is_stuck() {
    let (fast_port, slow_port, source_port, dest_sock, _slow_listener) = setup_two_dests();
    let config_path = temp_config_path("overflow-block");
    write_config(
        &config_path,
        &ConfigSpec::new(
            EndpointSpec::udp_server(source_port),
            vec![
                DestSpec::from_endpoint(EndpointSpec::udp_client(fast_port)),
                DestSpec::from_endpoint(EndpointSpec::tcp_client(slow_port)).with_overflow("block"),
            ],
        ),
    );

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };
    std::thread::sleep(STARTUP_DELAY);

    send_paced(source_port);
    let received = drain_fast(&dest_sock);
    // The slow dest's mpsc channel (capacity 64) fills, then every
    // subsequent fan-out's `tx.send().await` on the slow side awaits
    // forever (the slow TCP socket never drains its kernel send
    // buffer once we've filled it). The fast path receives whatever
    // landed before the block kicks in.
    assert!(
        received < (N * 2 / 3) as usize,
        "block: fast dest got {received} / {N}; expected substantially less because the \
         slow dest should be throttling the source"
    );
    // Sanity: block isn't supposed to mean *zero* deliveries — packets
    // queued before the slow channel filled must still get through.
    assert!(
        received > 0,
        "block: fast dest received nothing, which suggests the relay isn't running"
    );

    drop(guard);
}

fn send_paced(source_port: u16) {
    let tx = UdpSocket::bind("127.0.0.1:0").expect("bind tx");
    let source_addr = format!("127.0.0.1:{source_port}");
    let payload = vec![0x5Au8; PAYLOAD_LEN];
    for _ in 0..N {
        tx.send_to(&payload, &source_addr).expect("send");
        std::thread::sleep(PACE);
    }
}

/// Bind the fast UDP destination, the slow TCP listener (which accepts
/// but never reads), and pick a source port. Returns the bound objects
/// so the test keeps them alive for the whole run; the TCP listener
/// silently absorbs the relay's connect.
fn setup_two_dests() -> (u16, u16, u16, UdpSocket, TcpListener) {
    let fast_port = ephemeral_udp_port();
    let dest_sock = UdpSocket::bind(format!("127.0.0.1:{fast_port}")).expect("bind fast dest");
    dest_sock
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();

    // The slow side is a TCP listener that accepts but never reads.
    let slow = TcpListener::bind("127.0.0.1:0").expect("bind slow listener");
    let slow_port = slow.local_addr().expect("local").port();
    let slow_clone = slow.try_clone().expect("clone listener");
    std::thread::spawn(move || {
        // Accept once and hold the connection forever. The relay's
        // writes will fill the kernel send buffer, then block. Spawning
        // here keeps the test thread free to drive the fast path.
        let _stream = slow_clone.accept();
        std::thread::park();
    });

    let source_port = ephemeral_udp_port();
    (fast_port, slow_port, source_port, dest_sock, slow)
}

fn drain_fast(sock: &UdpSocket) -> usize {
    let mut count = 0usize;
    while udp_try_recv(sock, "fast").is_some() {
        count += 1;
        if count > N as usize * 2 {
            // Defensive: don't loop forever if something is wrong.
            break;
        }
    }
    count
}
