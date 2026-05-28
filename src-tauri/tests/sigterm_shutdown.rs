// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026-present Patrick S Connallon

//! systemd, Kubernetes, and Docker all stop processes with SIGTERM. This
//! test asserts the headless binary treats SIGTERM as a graceful-shutdown
//! signal indistinguishable from SIGINT: exit code 0, with the per-endpoint
//! stats reporter emitting one final `statistics` line before the process
//! returns.

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CONFIG: &str = r#"
[general]
log_level = "info"
stats_interval_secs = 60

[source]
protocol = "udp"
mode = "server"
address = "127.0.0.1:0"

[[destinations]]
protocol = "udp"
mode = "client"
address = "127.0.0.1:1"
"#;

#[test]
fn sigterm_triggers_graceful_shutdown() {
    let mut config_path = std::env::temp_dir();
    config_path.push(format!("nexthop-sigterm-{}.toml", std::process::id()));
    std::fs::File::create(&config_path)
        .and_then(|mut f| f.write_all(CONFIG.as_bytes()))
        .expect("write config");

    let bin = env!("CARGO_BIN_EXE_nexthop");
    let child = Command::new(bin)
        .arg("--no-gui")
        .arg("--config")
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn nexthop");

    // Wait for the SIGTERM handler to be installed before signaling.
    // Without this the signal can race the default disposition and the
    // child exits 143 instead of running the graceful path.
    std::thread::sleep(Duration::from_millis(750));

    let pid = child.id() as i32;
    let rc = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(
        rc,
        0,
        "kill(SIGTERM) failed: {}",
        std::io::Error::last_os_error()
    );

    let out = wait_with_timeout(child, Duration::from_secs(10))
        .expect("nexthop did not exit within 10s of SIGTERM");

    let _ = std::fs::remove_file(&config_path);

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        out.status.success(),
        "expected exit 0, got {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        out.status,
    );
    assert!(
        combined.contains("SIGTERM"),
        "expected shutdown log to name SIGTERM, got:\n{combined}"
    );
    assert!(
        combined.contains("statistics"),
        "expected a final stats summary before exit, got:\n{combined}"
    );
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}
