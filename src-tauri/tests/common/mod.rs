// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! Shared helpers for the cross-platform subprocess integration tests
//! under `src-tauri/tests/`.
//!
//! Cargo treats this `common` directory specially — because the file
//! is `tests/common/mod.rs` (not `tests/common.rs`), it is **not**
//! compiled as a separate integration test crate. Each scenario file
//! at `tests/<scenario>.rs` declares `mod common;` and reuses these
//! helpers.

#![allow(dead_code)]

use std::io::Write;
use std::net::{TcpListener, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Pick an OS-assigned free port on `127.0.0.1` and immediately drop
/// the binder so the relay can rebind. There is a small race window
/// between drop and rebind, but for a single-test process on loopback
/// it is rare enough in practice that we accept it instead of running
/// a coordination dance.
pub fn ephemeral_udp_port() -> u16 {
    let s = UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral UDP");
    let p = s.local_addr().expect("local_addr").port();
    drop(s);
    p
}

pub fn ephemeral_tcp_port() -> u16 {
    let s = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral TCP");
    let p = s.local_addr().expect("local_addr").port();
    drop(s);
    p
}

/// A description of one endpoint for [`write_config`]. Used for both
/// the source and each destination so the tests can mix protocols.
pub struct EndpointSpec {
    pub protocol: &'static str, // "tcp" | "udp"
    pub mode: &'static str,     // "server" | "client"
    pub address: String,
}

impl EndpointSpec {
    pub fn tcp_server(port: u16) -> Self {
        Self {
            protocol: "tcp",
            mode: "server",
            address: format!("127.0.0.1:{port}"),
        }
    }
    pub fn tcp_client(port: u16) -> Self {
        Self {
            protocol: "tcp",
            mode: "client",
            address: format!("127.0.0.1:{port}"),
        }
    }
    pub fn udp_server(port: u16) -> Self {
        Self {
            protocol: "udp",
            mode: "server",
            address: format!("127.0.0.1:{port}"),
        }
    }
    pub fn udp_client(port: u16) -> Self {
        Self {
            protocol: "udp",
            mode: "client",
            address: format!("127.0.0.1:{port}"),
        }
    }
}

/// One `[[destinations]]` block for [`write_config`].
pub struct DestSpec {
    pub endpoint: EndpointSpec,
    pub overflow_policy: Option<&'static str>, // None → drop_newest default
    pub rate_limit: Option<(u64, u64)>,        // bytes_per_second, burst
    pub transforms: Vec<TransformSpec>,
}

pub enum TransformSpec {
    DropSmallerThan { n_bytes: usize },
}

impl DestSpec {
    pub fn from_endpoint(endpoint: EndpointSpec) -> Self {
        Self {
            endpoint,
            overflow_policy: None,
            rate_limit: None,
            transforms: Vec::new(),
        }
    }
    pub fn with_overflow(mut self, policy: &'static str) -> Self {
        self.overflow_policy = Some(policy);
        self
    }
    pub fn with_rate_limit(mut self, bps: u64, burst: u64) -> Self {
        self.rate_limit = Some((bps, burst));
        self
    }
    pub fn with_drop_smaller_than(mut self, n_bytes: usize) -> Self {
        self.transforms
            .push(TransformSpec::DropSmallerThan { n_bytes });
        self
    }
}

pub struct ConfigSpec {
    pub source: EndpointSpec,
    pub destinations: Vec<DestSpec>,
    pub log_level: &'static str,
    pub stats_interval_secs: u64,
    pub global_rate_limit: Option<(u64, u64)>,
}

impl ConfigSpec {
    pub fn new(source: EndpointSpec, destinations: Vec<DestSpec>) -> Self {
        Self {
            source,
            destinations,
            log_level: "warn",
            stats_interval_secs: 3600,
            global_rate_limit: None,
        }
    }
    pub fn with_log_level(mut self, level: &'static str) -> Self {
        self.log_level = level;
        self
    }
    pub fn with_global_rate_limit(mut self, bps: u64, burst: u64) -> Self {
        self.global_rate_limit = Some((bps, burst));
        self
    }
}

pub fn write_config(path: &std::path::Path, cfg: &ConfigSpec) {
    let mut out = format!(
        r#"[general]
log_level = "{}"
stats_interval_secs = {}
channel_capacity = 64

[source]
protocol = "{}"
mode = "{}"
address = "{}"
"#,
        cfg.log_level,
        cfg.stats_interval_secs,
        cfg.source.protocol,
        cfg.source.mode,
        cfg.source.address,
    );

    if let Some((bps, burst)) = cfg.global_rate_limit {
        out.push_str(&format!(
            "\n[rate_limit]\nbytes_per_second = {bps}\nburst_size = {burst}\n"
        ));
    }

    for d in &cfg.destinations {
        out.push_str(&format!(
            "\n[[destinations]]\nprotocol = \"{}\"\nmode = \"{}\"\naddress = \"{}\"\n",
            d.endpoint.protocol, d.endpoint.mode, d.endpoint.address,
        ));
        if let Some(p) = d.overflow_policy {
            out.push_str(&format!("overflow_policy = \"{p}\"\n"));
        }
        if let Some((bps, burst)) = d.rate_limit {
            out.push_str(&format!(
                "rate_limit = {{ bytes_per_second = {bps}, burst_size = {burst} }}\n"
            ));
        }
        for t in &d.transforms {
            match t {
                TransformSpec::DropSmallerThan { n_bytes } => {
                    out.push_str(&format!(
                        "\n[[destinations.transforms]]\ntype = \"drop_smaller_than\"\nn_bytes = {n_bytes}\n"
                    ));
                }
            }
        }
    }

    let mut f = std::fs::File::create(path).expect("create config");
    f.write_all(out.as_bytes()).expect("write config");
    // On Windows in particular notify can coalesce back-to-back writes
    // if we don't flush + close explicitly here.
    drop(f);
}

/// Spawn the headless relay binary. Stdout / stderr are piped so they
/// don't spam the test runner's console; the [`ChildGuard`] drops the
/// pipes when the test ends.
pub fn spawn_relay(config_path: &std::path::Path) -> Child {
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

/// RAII cleanup for a spawned relay + its temp config file.
///
/// `Child::kill()` sends `SIGKILL` on Unix and `TerminateProcess` on
/// Windows. That is fine for test teardown — we never need a graceful
/// shutdown after the assertions have run.
pub struct ChildGuard {
    pub child: Option<Child>,
    pub config: std::path::PathBuf,
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

/// Try one non-blocking-ish UDP recv; returns `None` on `WouldBlock` /
/// `TimedOut` so callers can assert "did not arrive" without false
/// positives from spurious errors. Caller must set a read timeout on
/// the socket.
pub fn udp_try_recv(sock: &UdpSocket, label: &str) -> Option<Vec<u8>> {
    let mut buf = [0u8; 4096];
    match sock.recv_from(&mut buf) {
        Ok((n, _)) => Some(buf[..n].to_vec()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => None,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => None,
        Err(e) => panic!("{label}: recv error: {e}"),
    }
}

pub fn udp_expect(sock: &UdpSocket, expected: &[u8], label: &str) {
    let got = udp_try_recv(sock, label)
        .unwrap_or_else(|| panic!("{label}: expected to receive {expected:?}, got nothing"));
    assert_eq!(&got[..], expected, "{label}: payload mismatch");
}

/// Build a unique temp-file path for a test config. The PID + a
/// caller-provided label keep concurrent test invocations from
/// stomping each other.
pub fn temp_config_path(label: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("nexthop-{}-{}.toml", label, std::process::id()));
    p
}

/// Default startup wait. The relay binds + spawns its source / dest
/// tasks in well under a second on every CI runner we use, but this is
/// the upper bound we trust.
pub const STARTUP_DELAY: Duration = Duration::from_secs(1);

/// Wait window after rewriting the config file. The relay's `notify`
/// watcher has a 200 ms debounce; we leave plenty of slack for FS
/// event jitter and the apply_hot_reload async work.
pub const HOT_RELOAD_DELAY: Duration = Duration::from_millis(1500);
