# nexthop Manual

A raw TCP/UDP relay with cross-protocol forwarding, multicast support, per-destination
back-pressure, rate limiting, live config reload, and an optional graphical interface.

---

## Command-line options

```
nexthop [OPTIONS]
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--config <FILE>` | `-c` | `nexthop.toml` | Path to the TOML configuration file. |
| `--log-format <FORMAT>` | — | `text` | Log output format. `text` emits human-readable lines; `json` emits newline-delimited JSON suitable for Loki, Datadog, and similar aggregators. |
| `--no-gui` | — | *(GUI enabled)* | Disable the graphical interface and run in headless / command-line mode. |
| `--help` | `-h` | — | Print help and exit. |
| `--version` | `-V` | — | Print version and exit. |

### Examples

```sh
# Launch with the GUI (default)
nexthop --config nexthop.toml

# Use a non-default config path
nexthop --config /etc/relay/production.toml

# Headless mode — no GUI, logs to stdout
nexthop --no-gui --config /etc/relay/production.toml

# Emit JSON logs for a log aggregator (headless)
nexthop --no-gui --log-format json

# Both together
nexthop --no-gui -c /etc/relay/production.toml --log-format json
```

The log level is controlled by the `general.log_level` config key (or the `RUST_LOG`
environment variable, which takes precedence).

---

## Graphical interface

By default nexthop opens a native GUI window. The GUI has three tabs accessible from
the top bar:

### Configuration tab (⚙)

Provides a form-based editor for every config field. Changes are applied by clicking
**Save Configuration**, which writes the TOML file to disk. The relay must be stopped
before configuration can be saved.

- **Start / Stop** — The top-right area shows the relay's current state (● Running /
  ● Stopped) and a **▶ Start** or **■ Stop** button.
- **Data Source** — Cast mode, protocol, mode, IP version, IP address, port. Multicast
  and reconnect fields appear only when relevant.
- **Data Destinations** — A table of destination rows (Cast, Mode, Proto, IP Ver, IP
  Address, Port, Overflow, Name). Use **+ Add Destination** to add rows; the **✕** button
  removes a row. Advanced multicast/reconnect settings appear in a collapsible section
  below the table.
- **General Settings** — Log level, stats interval, channel capacity, max payload,
  health port.
- **Rate Limit** — Enable checkbox reveals bytes/second and burst size fields.
- **Status bar** — A dismissible status bar at the bottom shows success and error
  messages from save/start/stop operations.

### Monitoring tab (📊)

Displays live per-endpoint statistics while the relay is running. If the relay is
stopped the tab shows a placeholder message.

- **Source card** — RX bytes, message count, active/total connections, errors, uptime.
- **Destination cards** — TX bytes, message count, active/total connections, errors,
  dropped packets (queue overflow), uptime.
- **Refresh interval** — A drag-value control (top-right) adjusts how often the
  statistics panels repaint (0.1 s – 60 s).

### Preferences tab (🎨)

Visual preferences. The toggle takes effect immediately; click **Save Preferences** to
persist the choice across restarts.

| Control | Description |
|---------|-------------|
| Dark mode | Switches the UI between dark and light themes. |

Preferences are stored in **`preferences.toml`** in the same directory as the config
file. The file is created automatically on first save.

---

## Configuration file

The file is [TOML](https://toml.io). It has three top-level sections:

| Section | Required | Description |
|---------|----------|-------------|
| `[general]` | yes | Global process settings. |
| `[source]` | yes | The single inbound endpoint. |
| `[rate_limit]` | no | Token-bucket rate limiter applied to all inbound traffic. Omit the section entirely for unlimited throughput. |
| `[[destinations]]` | yes (≥ 1) | One or more outbound endpoints. Repeat the header for each destination. |

---

### `[general]`

```toml
[general]
log_level           = "info"   # optional — default: "info"
stats_interval_secs = 30       # optional — default: 30
channel_capacity    = 1024     # optional — default: 1024
max_payload_size    = 65535    # optional — default: 65535
health_port         = 9090     # optional — omit to disable
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `log_level` | string | `"info"` | Minimum log level. Accepted values: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`. The `RUST_LOG` environment variable overrides this value. |
| `stats_interval_secs` | integer | `30` | How often (in seconds) each endpoint logs a statistics summary. A final summary is also logged on shutdown. |
| `channel_capacity` | integer | `1024` | Depth of the internal per-destination queue (number of payloads). When the queue is full the destination's `overflow_policy` determines what happens. |
| `max_payload_size` | integer | `65535` | Maximum single-read payload in bytes. TCP reads and UDP datagrams larger than this are dropped and counted as errors. Must be > 0. |
| `health_port` | integer | *(disabled)* | TCP port for the built-in HTTP health/stats server. When set, the server listens on `0.0.0.0:<health_port>`. See [Health endpoint](#health-endpoint) below. Omit the key to disable completely. |

---

### `[source]`

The single endpoint from which the relay reads data.

```toml
[source]
name                      = "ingest"      # optional
protocol                  = "udp"         # required: tcp | udp
mode                      = "server"      # required: server | client
address                   = "0.0.0.0:10000" # required
cast_mode                 = "unicast"     # optional: unicast | broadcast | multicast
multicast_interface       = "0.0.0.0"     # optional (multicast IPv4 only)
multicast_interface_index = 0             # optional (multicast IPv6 only)
multicast_ttl             = 2             # optional (multicast only)
reconnect_delay_ms        = 2000          # optional (client mode only)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(address string)* | Human-readable label used in log output and stats reports. Defaults to the `address` value when omitted. |
| `protocol` | string | *(required)* | Transport protocol. `"tcp"` or `"udp"`. |
| `mode` | string | *(required)* | `"server"` — bind and accept/receive incoming connections. `"client"` — connect outward to `address` and reconnect on disconnect. |
| `address` | string | *(required)* | `host:port` to bind (server mode) or connect to (client mode). Use `0.0.0.0` to listen on all interfaces. |
| `cast_mode` | string | `"unicast"` | UDP delivery mode. `"unicast"` — standard point-to-point. `"broadcast"` — receive from the subnet broadcast address. `"multicast"` — join a multicast group. Ignored for TCP. |
| `multicast_interface` | string | `"0.0.0.0"` | IPv4 address of the local NIC to use when joining a multicast group. Ignored unless `cast_mode = "multicast"`. Must be an IP address, not an interface name. |
| `multicast_interface_index` | integer | `0` | Interface index for IPv6 multicast group membership. Ignored unless `cast_mode = "multicast"` with an IPv6 address. |
| `multicast_ttl` | integer | `16` | IP TTL for multicast datagrams. Ignored unless `cast_mode = "multicast"`. |
| `reconnect_delay_ms` | integer | `2000` | Milliseconds to wait before reconnecting after a disconnect or connection failure. Only used in `mode = "client"`. |

---

### `[rate_limit]`

Applies a global token-bucket rate limit to all bytes read from the source before they
are forwarded. Omit the entire section for unlimited throughput.

```toml
[rate_limit]
bytes_per_second = 10485760   # required: 10 MB/s
burst_size       = 131072     # optional: default 131072 (128 KB)
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bytes_per_second` | integer | *(required)* | Sustained throughput cap in bytes per second. Must be > 0. |
| `burst_size` | integer | `131072` | Maximum burst the bucket allows above the sustained rate (bytes). Traffic in excess blocks until tokens refill. |

The rate limiter uses an atomic compare-and-swap token bucket. It is updated live when
the config is hot-reloaded without restarting the process.

---

### `[[destinations]]`

One entry per outbound endpoint. Repeat the `[[destinations]]` header for each one.
All destination fields are the same as source fields with the addition of
`overflow_policy`.

```toml
[[destinations]]
name                      = "tcp-backend"
protocol                  = "tcp"
mode                      = "client"
address                   = "127.0.0.1:20000"
reconnect_delay_ms        = 3000
overflow_policy           = "drop_newest"
```

```toml
[[destinations]]
name      = "udp-mirror"
protocol  = "udp"
mode      = "server"
address   = "0.0.0.0:20001"
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `name` | string | *(address string)* | Label used in logs and stats. |
| `protocol` | string | *(required)* | `"tcp"` or `"udp"`. |
| `mode` | string | *(required)* | `"client"` — relay connects outward to `address` and reconnects on failure. `"server"` — relay binds and fans out to every connected peer; peers connect inward. |
| `address` | string | *(required)* | `host:port` to connect to (client) or bind (server). |
| `cast_mode` | string | `"unicast"` | `"unicast"`, `"broadcast"`, or `"multicast"`. Broadcast and multicast require `protocol = "udp"`. |
| `multicast_interface` | string | `"0.0.0.0"` | IPv4 NIC address for multicast. Must be an IP address, not an interface name. |
| `multicast_interface_index` | integer | `0` | Interface index for IPv6 multicast. |
| `multicast_ttl` | integer | `16` | TTL for multicast datagrams. |
| `reconnect_delay_ms` | integer | `2000` | Reconnect pause in milliseconds. Only used in `mode = "client"`. |
| `overflow_policy` | string | `"drop_newest"` | What to do when this destination's internal queue is full. `"drop_newest"` discards the incoming packet and keeps the source running. `"block"` applies back-pressure all the way to the source until the queue drains. Each destination has an independent policy and queue. |

#### Protocol / mode combinations

| Protocol | Mode | Behavior |
|----------|------|----------|
| `tcp` | `client` | Relay connects to `address`; reconnects on error or disconnect. |
| `tcp` | `server` | Relay listens on `address`; fans out every packet to all connected peers. A reconnecting peer evicts its stale entry automatically. |
| `udp` | `client` | Relay sends datagrams to `address` (unicast, broadcast, or multicast). |
| `udp` | `server` | Relay binds `address`; any peer that sends a datagram to the port is registered. The relay then forwards to all registered peers. |

---

## Hot reload

The relay watches the config file for changes. When a modification is detected the
following fields take effect immediately **without restarting** any tasks:

- `[rate_limit]` — the new limiter (or no limiter if the section is removed) is applied
  on the very next packet.

Changes to `[source]`, `[[destinations]]`, and `[general]` (other than `rate_limit`)
require a full process restart. The relay logs a warning when it detects such changes in
a reload.

---

## Health endpoint

When `general.health_port` is set, a minimal HTTP server starts on `0.0.0.0:<port>`.

| Path | Method | Response |
|------|--------|----------|
| `/health` | `GET` | `200 OK` — `{"status":"ok"}`. Always succeeds while the process is alive. |
| `/stats` | `GET` | `200 OK` — JSON array of counter snapshots, one object per endpoint (source first, then each destination in config order). |
| `/metrics` | `GET` | `200 OK` — Prometheus text-exposition format (`text/plain; version=0.0.4`). Same counters as `/stats`, labeled by endpoint. |

### `/stats` response shape

```json
[
  {
    "label":              "source(ingest)",
    "local_addr":         "0.0.0.0:10000",
    "peer_addr":          "",
    "uptime_s":           120,
    "rx_bytes":           4096000,
    "tx_bytes":           0,
    "messages":           0,
    "active_connections": 1,
    "total_connections":  3,
    "errors":             0,
    "dropped":            0
  },
  {
    "label":              "dest(tcp-backend)",
    "local_addr":         "",
    "peer_addr":          "10.0.0.1:20000",
    "uptime_s":           120,
    "rx_bytes":           0,
    "tx_bytes":           4096000,
    "messages":           8000,
    "active_connections": 1,
    "total_connections":  1,
    "errors":             0,
    "dropped":            0
  }
]
```

All counter fields are unsigned 64-bit integers. `tx_bytes` on a destination counts
bytes successfully written; `rx_bytes` on the source counts bytes read from the wire.

### `/metrics` response shape

Standard Prometheus [text-exposition](https://github.com/prometheus/docs/blob/main/content/docs/instrumenting/exposition_formats.md)
format. Every metric carries a single `endpoint` label whose value is the
display name used in the logs (e.g. `source(ingest)`, `dest(tcp-backend)`).

| Metric | Type | Notes |
|--------|------|-------|
| `nexthop_rx_bytes_total` | counter | Bytes read from the wire by this endpoint. |
| `nexthop_tx_bytes_total` | counter | Bytes successfully written. |
| `nexthop_messages_total` | counter | Discrete messages relayed. |
| `nexthop_errors_total` | counter | Total error events. |
| `nexthop_dropped_total` | counter | Packets dropped (queue overflow). |
| `nexthop_connections_opened_total` | counter | Cumulative connections opened. |
| `nexthop_active_connections` | gauge | Currently open connections. |
| `nexthop_uptime_seconds` | gauge | Seconds since the endpoint task started. |

Example scrape config:

```yaml
scrape_configs:
  - job_name: nexthop
    static_configs:
      - targets: ['nexthop-host:9090']
```

---

## Full annotated example

```toml
[general]
log_level           = "info"
stats_interval_secs = 60
channel_capacity    = 2048
max_payload_size    = 65535
health_port         = 9090       # expose /health and /stats on port 9090

[source]
name     = "ingest"
protocol = "udp"
mode     = "server"
address  = "0.0.0.0:10000"

[rate_limit]
bytes_per_second = 10485760      # 10 MB/s
burst_size       = 262144        # 256 KB burst

# Fan out to a persistent TCP backend (relay connects out, reconnects on drop).
[[destinations]]
name               = "tcp-backend"
protocol           = "tcp"
mode               = "client"
address            = "10.0.0.1:20000"
reconnect_delay_ms = 3000
overflow_policy    = "block"     # back-pressure source rather than drop

# Also mirror to a local UDP port (any peer that sends a probe packet is registered).
[[destinations]]
name             = "udp-mirror"
protocol         = "udp"
mode             = "server"
address          = "0.0.0.0:20001"
overflow_policy  = "drop_newest"

# Forward to a multicast group for LAN distribution.
[[destinations]]
name                = "multicast-group"
protocol            = "udp"
mode                = "client"
address             = "239.1.1.1:30000"
cast_mode           = "multicast"
multicast_interface = "0.0.0.0"
multicast_ttl       = 4
overflow_policy     = "drop_newest"

# Forward to an IPv6 multicast group.
[[destinations]]
name                      = "multicast-ipv6"
protocol                  = "udp"
mode                      = "client"
address                   = "[ff02::1]:30001"
cast_mode                 = "multicast"
multicast_interface_index = 2             # OS interface index (e.g. from `ip link`)
multicast_ttl             = 4
overflow_policy           = "drop_newest"
```
