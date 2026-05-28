# nexthop

[![Rust CI](https://github.com/SupremeCommanderHedgehog/nexthop/actions/workflows/rust.yml/badge.svg)](https://github.com/SupremeCommanderHedgehog/nexthop/actions/workflows/rust.yml)
[![Latest release](https://img.shields.io/github/v/release/SupremeCommanderHedgehog/nexthop?sort=semver)](https://github.com/SupremeCommanderHedgehog/nexthop/releases)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE.md)
[![Rust 1.77+](https://img.shields.io/badge/rust-1.77%2B-orange.svg)](#requirements)

A raw TCP/UDP relay with cross-protocol forwarding, multicast/broadcast support,
per-destination back-pressure, token-bucket rate limiting, and live config reload.
Ships as a native desktop app (Tauri v2 + React) with an optional headless mode
for running as a service or in CI pipelines.

---

## Features

- **Cross-protocol forwarding** — receive on TCP and forward to UDP, or vice versa
- **Unicast, broadcast, and multicast** — IPv4 and IPv6 multicast group membership
- **Multiple destinations** — fan out one source to many endpoints simultaneously
- **Per-destination overflow policy** — `drop_newest` (default) or `block` (back-pressure)
- **Token-bucket rate limiting** — configurable bytes/sec cap with burst allowance
- **Live config reload** — rate limiter updates take effect on the next packet without a restart
- **Health endpoint** — optional HTTP `/health`, `/stats`, and Prometheus `/metrics` server for monitoring
- **GUI + headless** — full graphical interface for interactive use; `--no-gui` for server deployments
- **Dark / light theme**

---

## Requirements

| Tool | Version |
|------|---------|
| Rust + Cargo | 1.77+ |
| Node.js | 18+ |
| npm | 9+ |
| Tauri CLI | v2 (installed via npm) |

### Platform prerequisites

- **Windows** — WebView2 runtime (pre-installed on Windows 11; installer available from Microsoft)
- **macOS** — Xcode Command Line Tools
- **Linux** — `libwebkit2gtk-4.1`, `libgtk-3`, `libayatana-appindicator3` (or `libappindicator3`)

---

## Building

```sh
# Install frontend dependencies (first time only)
npm install

# Development build — hot-reload frontend + live Tauri window
npm run tauri:dev

# Production build (output in src-tauri/target/release/)
npm run tauri:build
```

Both scripts pass `--features` to the Tauri CLI so the `gui` /
`custom-protocol` cargo features stay on through the
`--no-default-features` flag the Tauri CLI applies internally. See
[#71](https://github.com/SupremeCommanderHedgehog/nexthop/issues/71)
for background.

---

## Running

### GUI mode (default)

```sh
nexthop --config nexthop.toml
```

Opens the desktop window. The **Configuration** tab loads the config file and lets
you start/stop the relay. The **Monitoring** tab shows live per-endpoint statistics.

### Headless mode

```sh
nexthop --no-gui --config /etc/relay/production.toml

# JSON-structured logs for Loki / Datadog / etc.
nexthop --no-gui --log-format json --config production.toml
```

The relay starts immediately and logs to stdout. Ctrl-C triggers a graceful shutdown.

### CLI reference

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--config <FILE>` | `-c` | `nexthop.toml` | Path to the TOML config file |
| `--log-format <FORMAT>` | — | `text` | `text` or `json` |
| `--no-gui` | — | *(GUI on)* | Run headless without opening a window |
| `--help` | `-h` | — | Print help |
| `--version` | `-V` | — | Print version |

---

## Quick-start config

```toml
[general]
log_level           = "info"
stats_interval_secs = 30
channel_capacity    = 1024
max_payload_size    = 65535

[source]
protocol = "udp"
mode     = "server"
address  = "0.0.0.0:10000"

[[destinations]]
protocol        = "udp"
mode            = "client"
address         = "127.0.0.1:20000"
overflow_policy = "drop_newest"
```

Save as `nexthop.toml` and run `nexthop`. See [MANUAL.md](MANUAL.md) for the full
configuration reference, including multicast, rate limiting, and the health endpoint.

---

## Testing

```sh
# Rust unit tests
cd src-tauri && cargo test

# TypeScript unit tests
npm test

# TypeScript tests in watch mode
npm run test:watch
```

---

## Developing

This repo ships a [`pre-commit`](https://pre-commit.com/) configuration that
runs gitleaks plus `cargo fmt --check` and `cargo clippy -- -D warnings` on
every commit that touches Rust files. Install it once per checkout so format
or lint issues are caught locally before they reach CI:

```sh
pip install pre-commit       # one-time, system-wide
pre-commit install           # per-checkout, installs the git hook

# Rescan the whole tree on demand:
pre-commit run --all-files
```

The clippy hook only fires when `*.rs` files are staged, so non-Rust commits
are not slowed down.

---

## SBOM

Each tagged release ships with two Software Bill of Materials files
attached to the GitHub release page:

| File | Format |
|------|--------|
| `nexthop-vX.Y.Z.spdx.json` | [SPDX 2.3 JSON](https://spdx.dev/) |
| `nexthop-vX.Y.Z.cdx.json` | [CycloneDX 1.5 JSON](https://cyclonedx.org/) |

Both are generated by `anchore/syft` in the release workflow and cover
the Rust and npm dependency graphs of the tagged commit. Either format
is consumable by standard supply-chain tooling — e.g. Grype, Trivy,
Dependency-Track.

---

## Compatibility

[COMPATIBILITY.md](COMPATIBILITY.md) defines what counts as a breaking
change. The config schema, CLI flags, HTTP endpoints, and exit codes
are the contractual surfaces; internal Rust APIs, log text, and the
GUI are not. Read this before you write tooling against `nexthop`.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for dev setup, commit conventions,
signing requirements, and the bar for getting changes merged.

---

## License

GPL-3.0-or-later — see [LICENSE.md](LICENSE.md) for details.
