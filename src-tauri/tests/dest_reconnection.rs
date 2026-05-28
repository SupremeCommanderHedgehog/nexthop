// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Scenario 3 of #24: destination reconnection after the peer drops.
//!
//! A TCP-client destination connects to a server, the server closes
//! the connection, and the relay must reconnect on its own after the
//! configured `reconnect_delay_ms`. This is the property that keeps
//! the relay self-healing across rolling restarts of downstream
//! services.
//!
//! Verification strategy: bind a TCP listener as the "downstream",
//! count `accept()` events. Spawn the relay pointing at us. We accept
//! once, immediately close, and assert that we receive a second
//! `accept()` within a window slightly larger than
//! `reconnect_delay_ms`. A third accept proves the cycle is durable,
//! not a one-shot fluke.

mod common;

use common::*;
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn destination_tcp_client_reconnects_after_peer_closes() {
    // Short reconnect delay so the whole test finishes in well under
    // 5 seconds without making the assertion windows too tight to
    // tolerate CI scheduling jitter.
    const RECONNECT_DELAY_MS: u64 = 300;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let dest_port = listener.local_addr().expect("local").port();
    let source_port = ephemeral_udp_port();

    // Run the accept loop on a dedicated thread. Each successful
    // accept immediately drops the stream (which closes the connection
    // from this side) and signals the test thread with the wall time
    // of that accept.
    let (tx, rx) = mpsc::channel::<Instant>();
    let listener_thread = std::thread::spawn(move || {
        for _ in 0..3 {
            match listener.accept() {
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

    let config_path = temp_config_path("dest-reconnect");
    let mut cfg = ConfigSpec::new(
        EndpointSpec::udp_server(source_port),
        vec![DestSpec::from_endpoint(EndpointSpec::tcp_client(dest_port))],
    );
    // Patch in the custom reconnect delay. We extend write_config with
    // an inline edit rather than a builder field because this is the
    // only test that needs it.
    write_config(&config_path, &cfg);
    inject_reconnect_delay(&config_path, RECONNECT_DELAY_MS);
    let _ = &mut cfg;

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    // Drive UDP packets through the source continuously. Without
    // traffic the relay's TCP-client dest task never *tries* to write
    // to the closed peer, so it never discovers that the connection
    // is dead — and therefore never reconnects. Real deployments
    // always have traffic; this thread emulates that.
    let traffic_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_for_thread = std::sync::Arc::clone(&traffic_stop);
    let traffic_thread = std::thread::spawn(move || {
        let sock = std::net::UdpSocket::bind("127.0.0.1:0").expect("bind tx");
        let addr = format!("127.0.0.1:{source_port}");
        while !stop_for_thread.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = sock.send_to(b"heartbeat", &addr);
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    // First connection — the relay's TCP-client dest connects shortly
    // after the source bind succeeds. Bound the wait conservatively
    // for CI cold-start.
    let first = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("relay never made its first TCP connection");

    // Second connection — the listener closed immediately above, so
    // the relay should reconnect after ~RECONNECT_DELAY_MS. Allow up
    // to 2 s slack for the reconnect-sleep + accept + scheduling.
    let second = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("relay did not reconnect after the peer closed");

    let gap = second.duration_since(first);
    assert!(
        gap >= Duration::from_millis(RECONNECT_DELAY_MS / 2),
        "reconnect happened too quickly ({gap:?}); expected at least \
         half of the {RECONNECT_DELAY_MS}-ms reconnect delay"
    );
    assert!(
        gap <= Duration::from_secs(2),
        "reconnect took {gap:?}; far longer than the configured \
         {RECONNECT_DELAY_MS}-ms delay"
    );

    // Third connection — proves the reconnect loop is durable, not a
    // one-time hiccup.
    rx.recv_timeout(Duration::from_secs(2))
        .expect("relay stopped reconnecting after the second cycle");

    traffic_stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = traffic_thread.join();
    drop(guard);
    // The listener thread exits once it has accepted three times or
    // we drop the relay (which closes the listener's accept queue).
    let _ = listener_thread.join();
}

/// Append `reconnect_delay_ms = N` to every `[[destinations]]` block
/// in the file. Tiny TOML rewriter — good enough for tests, not for
/// production.
fn inject_reconnect_delay(path: &std::path::Path, ms: u64) {
    let s = std::fs::read_to_string(path).expect("read config");
    let mut out = String::with_capacity(s.len() + 64);
    for line in s.lines() {
        out.push_str(line);
        out.push('\n');
        if line.trim_start().starts_with("[[destinations]]") {
            out.push_str(&format!("reconnect_delay_ms = {ms}\n"));
        }
    }
    std::fs::write(path, out).expect("rewrite config");
}
