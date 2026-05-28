// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! End-to-end packet-flow coverage for hot-reload destination changes.
//!
//! The supervisor unit tests in `relay.rs` cover the diff and per-entry
//! atomic updates; these subprocess tests close the loop by exercising
//! the real stack: source bind → fan-out → destination delivery, plus
//! the notify-driven config watcher that triggers `apply_hot_reload`.
//!
//! Both tests are cross-platform: they bind only to `127.0.0.1` and use
//! the headless binary with stdout/stderr captured for diagnostics on
//! failure. UDP loopback works the same on Linux, macOS, and Windows.

use std::io::Write;
use std::net::UdpSocket;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Bind to port 0 to let the OS pick a free port, then drop the socket
/// so the relay can rebind to it. There's a small race where another
/// process could claim the port in between, but for a single-test
/// process on loopback this is rare enough in practice that it isn't
/// worth a full coordination protocol.
fn ephemeral_port() -> u16 {
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
    let port = sock.local_addr().expect("local_addr").port();
    drop(sock);
    port
}

fn write_config(path: &std::path::Path, source_port: u16, dest_ports: &[u16]) {
    let mut cfg = format!(
        r#"[general]
log_level = "warn"
stats_interval_secs = 3600
channel_capacity = 64

[source]
protocol = "udp"
mode = "server"
address = "127.0.0.1:{source_port}"
"#
    );
    for p in dest_ports {
        cfg.push_str(&format!(
            "\n[[destinations]]\nprotocol = \"udp\"\nmode = \"client\"\naddress = \"127.0.0.1:{p}\"\n"
        ));
    }
    let mut f = std::fs::File::create(path).expect("create config");
    f.write_all(cfg.as_bytes()).expect("write config");
    // notify on Windows in particular sometimes coalesces back-to-back
    // writes if we don't flush + close explicitly here.
    drop(f);
}

fn spawn_relay(config_path: &std::path::Path) -> Child {
    let bin = env!("CARGO_BIN_EXE_nexthop");
    Command::new(bin)
        .arg("--no-gui")
        .arg("--config")
        .arg(config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn nexthop")
}

fn try_recv(sock: &UdpSocket, label: &str) -> Option<Vec<u8>> {
    let mut buf = [0u8; 256];
    match sock.recv_from(&mut buf) {
        Ok((n, _)) => Some(buf[..n].to_vec()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => None,
        Err(e) => panic!("{label}: recv error: {e}"),
    }
}

fn expect_recv(sock: &UdpSocket, expected: &[u8], label: &str) {
    let got = try_recv(sock, label)
        .unwrap_or_else(|| panic!("{label}: expected to receive {expected:?}, got nothing"));
    assert_eq!(&got[..], expected, "{label}: payload mismatch");
}

/// Stop the relay cleanly when the test ends (passing or panicking).
/// `Child::kill()` sends SIGKILL on Unix and TerminateProcess on
/// Windows, which is fine for cleanup — we don't need a graceful
/// shutdown here.
struct ChildGuard {
    child: Option<Child>,
    config: std::path::PathBuf,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_file(&self.config);
    }
}

#[test]
fn hot_reload_add_destination_delivers_packets_to_both() {
    let source_port = ephemeral_port();
    let dest_a = UdpSocket::bind("127.0.0.1:0").expect("bind dest_a");
    let dest_b = UdpSocket::bind("127.0.0.1:0").expect("bind dest_b");
    dest_a
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    dest_b
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let port_a = dest_a.local_addr().unwrap().port();
    let port_b = dest_b.local_addr().unwrap().port();

    let mut config_path = std::env::temp_dir();
    config_path.push(format!("nexthop-hr-add-{}.toml", std::process::id()));
    write_config(&config_path, source_port, &[port_a]);

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    // Give the source time to bind. The relay's first log line is
    // "config hot-reload active" / "source starting"; we don't try to
    // parse those because piped stdout/stderr would need a reader
    // thread to keep the child unblocked. 1s is plenty for a debug
    // build on the slowest CI runner.
    std::thread::sleep(Duration::from_secs(1));

    let tx = UdpSocket::bind("127.0.0.1:0").expect("bind tx");
    let source_addr = format!("127.0.0.1:{source_port}");

    tx.send_to(b"phase-1", &source_addr).expect("send phase-1");
    expect_recv(&dest_a, b"phase-1", "dest_a phase-1");
    assert!(
        try_recv(&dest_b, "dest_b phase-1").is_none(),
        "dest_b should not yet exist"
    );

    // Hot-reload: add dest_b. The relay's notify watcher has a 200 ms
    // debounce; we wait considerably longer to absorb FS event jitter.
    write_config(&config_path, source_port, &[port_a, port_b]);
    std::thread::sleep(Duration::from_millis(1500));

    tx.send_to(b"phase-2", &source_addr).expect("send phase-2");
    expect_recv(&dest_a, b"phase-2", "dest_a phase-2");
    expect_recv(&dest_b, b"phase-2", "dest_b phase-2");

    drop(guard);
}

#[test]
fn hot_reload_remove_destination_stops_only_that_dest() {
    let source_port = ephemeral_port();
    let dest_a = UdpSocket::bind("127.0.0.1:0").expect("bind dest_a");
    let dest_b = UdpSocket::bind("127.0.0.1:0").expect("bind dest_b");
    dest_a
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    dest_b
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let port_a = dest_a.local_addr().unwrap().port();
    let port_b = dest_b.local_addr().unwrap().port();

    let mut config_path = std::env::temp_dir();
    config_path.push(format!("nexthop-hr-remove-{}.toml", std::process::id()));
    write_config(&config_path, source_port, &[port_a, port_b]);

    let guard = ChildGuard {
        child: Some(spawn_relay(&config_path)),
        config: config_path.clone(),
    };

    std::thread::sleep(Duration::from_secs(1));

    let tx = UdpSocket::bind("127.0.0.1:0").expect("bind tx");
    let source_addr = format!("127.0.0.1:{source_port}");

    // Both destinations active at this point.
    tx.send_to(b"phase-1", &source_addr).expect("send phase-1");
    expect_recv(&dest_a, b"phase-1", "dest_a phase-1");
    expect_recv(&dest_b, b"phase-1", "dest_b phase-1");

    // Hot-reload: drop dest_b.
    write_config(&config_path, source_port, &[port_a]);
    std::thread::sleep(Duration::from_millis(1500));

    tx.send_to(b"phase-2", &source_addr).expect("send phase-2");
    expect_recv(&dest_a, b"phase-2", "dest_a phase-2");
    assert!(
        try_recv(&dest_b, "dest_b phase-2").is_none(),
        "dest_b must not receive after removal"
    );

    drop(guard);
}
