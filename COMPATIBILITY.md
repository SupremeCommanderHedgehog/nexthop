# Compatibility promise

This document defines what counts as a breaking change for `nexthop`
and what does not. It is written so that the 1.0 cut is a mechanical
check, not a judgment call, and so that downstream integrators can
plan their upgrades.

Closes the policy work in
[#30](https://github.com/SupremeCommanderHedgehog/nexthop/issues/30).

---

## Status: pre-1.0

`nexthop` is still in the 0.x line. Until 1.0, this contract applies
loosely:

- **Minor** version bumps may include breaking changes to anything in
  this document. CHANGELOG.md calls them out.
- **Patch** version bumps are backwards-compatible only.

After 1.0, the surfaces enumerated under **"Frozen at 1.0"** below
are stable: any breaking change to them requires a **major** version
bump (2.0, 3.0, etc.). The surfaces enumerated under **"Explicitly
not promised"** can change in any release.

The 1.0 cut itself happens when at least one of these is true:

- An external user has a `nexthop.toml` you do not control.
- An external consumer scrapes `/stats` or `/metrics` from your
  deployment.

These are triggers, not deadlines.

---

## Frozen at 1.0

### TOML config (`nexthop.toml`)

Every key documented in `MANUAL.md`'s
[**Configuration**](MANUAL.md#configuration) section is part of the
contract:

- The set of accepted keys per table.
- The TOML type of each key (`integer`, `string`, table, array-of-tables,
  etc.).
- The default value of each key when omitted.
- The semantics — what each key controls, what valid values mean,
  what invalid values do.

Concretely, that means **all** of:

- `[general]` — `log_level`, `stats_interval_secs`,
  `channel_capacity`, `max_payload_size`, `health_port`,
  `health_bind_addr`.
- `[source]` — `name`, `protocol`, `mode`, `address`, `cast_mode`,
  `multicast_interface`, `multicast_interface_index`,
  `multicast_ttl`, `reconnect_delay_ms`.
- `[rate_limit]` — `bytes_per_second`, `burst_size`.
- `[[destinations]]` — every key the source table has, plus
  `overflow_policy`, `rate_limit` (inline-table form), and
  `transforms` (an ordered array-of-tables, each entry tagged with a
  `type` discriminator and carrying its own transform-specific
  fields; see `MANUAL.md`'s [Transforms](MANUAL.md#transforms)
  section for the catalogue).

Adding a **new** key with a sensible default is **not** a breaking
change. Changing the default value of an existing key **is**, because
it changes the behavior of an unmodified config.

### CLI flags

The headless binary's command line is part of the contract:

- `--config / -c <FILE>` (default: `nexthop.toml`).
- `--log-format <text|json>` (default: `text`).
- `--no-gui`.
- `--help / -h`.
- `--version / -V`.

Adding new optional flags is **not** breaking. Removing or renaming
an existing flag, or changing its short/long form, is.

### HTTP endpoints (`/health`, `/stats`, `/metrics`)

When `general.health_port` is set, the endpoints documented in
`MANUAL.md`'s [**Health endpoint**](MANUAL.md#health-endpoint)
section are part of the contract:

- The set of paths (`/health`, `/stats`, `/metrics`) and the methods
  they respond to (`GET`).
- The `Content-Type` of each response.
- The JSON object shape returned by `/stats`: every field name, its
  type, and its semantics. Per-endpoint snapshots are an array;
  ordering inside the array is source-first then destinations in
  config order.
- The Prometheus text-exposition layout returned by `/metrics`: the
  metric names, the label names (`endpoint`), the metric types
  (counter / gauge), and the units encoded in the metric names
  (`_bytes_total`, `_seconds`, etc.).
- HTTP status codes for the documented paths.

Adding **new** counters or **new** JSON fields is **not** breaking.
Renaming an existing one, removing it, changing its type, or
changing the meaning of an existing label is.

### `preferences.toml` (GUI mode)

When the desktop app saves user preferences, the on-disk schema is
part of the contract:

- The set of keys (currently: `dark_mode`).
- The TOML type of each.
- The default value applied when a key is missing.

The file's location on disk (the app data directory) and how it is
discovered are **not** frozen — both depend on the OS and on Tauri's
own resolution rules.

### Exit codes

The headless binary's documented exit codes are part of the contract:

| Code | Meaning |
|------|---------|
| `0`  | Relay started, ran, and shut down cleanly (via SIGINT, SIGTERM, or end-of-stream from a TCP-client source). |
| `1`  | Fatal error before or during relay startup — config failed to load or validate, source bind failed, or the relay terminated with an unrecoverable runtime error. The error is logged before exit. |
| `2`  | GUI mode requested (no `--no-gui`) but the binary was built without the `gui` feature. A fatal message is printed to stderr. |

Process supervisors (systemd, k8s, docker) can rely on these codes:
a `0` exit means "deliberately stopped", anything else means
"restart me".

---

## Explicitly not promised

### Internal Rust APIs (`nexthop_lib`)

`nexthop` ships as a binary. The library exists as a Cargo
implementation detail and to enable the out-of-tree fuzz crate; the
public-ish items inside `nexthop_lib::config` and `nexthop_lib::error`
exist only to let that fuzz crate compile.

Embedding `nexthop_lib` into another Rust crate is not a supported
integration mode. There is no semver contract on the internal Rust
types, modules, traits, or function signatures. They can be renamed
or reshaped in any release.

If you need a programmatic integration, run the headless binary as a
subprocess and consume `/stats` or `/metrics`. Those surfaces are
frozen.

### Log line text content

The relay logs through `tracing`. Two distinct contracts apply:

- **Stable**: the **structured field names** on every log event
  (e.g. `endpoint`, `dest`, `peer`, `rx_bytes`, `error`, `signal`,
  `bytes_per_second`). These appear identically in both `text` and
  `json` log formats, so downstream log pipelines that key on them
  (Loki labels, Datadog facets, etc.) continue to work across
  upgrades.
- **Not stable**: the surrounding human-readable message text, its
  capitalization, the relative order of log lines, the inclusion of
  new log lines at new events, and the precise wording of warnings
  and errors. Treat the text as for humans; treat the structured
  fields as for machines.

In JSON log format, the message text appears under the `message`
field but the contract still applies: parse on structured fields,
not on `message`.

### Tauri IPC command names and payloads

The desktop GUI communicates with its embedded backend through
Tauri's `invoke` mechanism. The command names
(`get_config`, `start_relay`, `get_stats`, etc.) and their argument
and return shapes are internal to the app — they can be renamed or
reshaped between any two releases.

External tools must not depend on them. Use the HTTP endpoints
instead.

### GUI layout, wording, and theme

Visual choices in the desktop app are not contractual:

- Tab order and labels.
- Colour scheme; dark / light theming details.
- Button text, modal copy, error message wording.
- Iconography.
- Screenshot- or pixel-based asserts will break.

The set of *capabilities* the GUI exposes (config edit, start/stop,
live stats view, preferences) is editorial direction, not an API.

---

## Deprecation policy

When a frozen surface needs to change incompatibly, the change
follows this sequence:

1. The replacement ships in some `N.x` release. The old surface
   keeps working unchanged in the same release. CHANGELOG marks it
   `Deprecated`.
2. On startup, the binary logs a deprecation notice at **WARN** level
   for every deprecated key, flag, or behavior the running config
   exercises. The notice names the replacement and the earliest
   version in which the old surface will be removed.
3. The old surface is removed **no earlier than the next major** —
   i.e. in some `(N+1).y` release. If something is deprecated in
   1.4, the earliest version that may remove it is 2.0.

The deprecation log line carries the structured field `deprecated =
true` so log pipelines can alert on it specifically.

Deprecation notices on a config key continue to fire on every
hot-reload, not only on first start, so an operator who edits the
file does not miss the warning.

---

## How to verify your integration before upgrading

| Surface | Quickest check |
|---------|----------------|
| `nexthop.toml` | Existing file parses with the new binary and the new binary's stats reporter logs the expected source/destination set. |
| CLI | Existing systemd / docker / k8s command lines launch without error. |
| `/health` | `curl http://localhost:<port>/health` returns `200 OK`. |
| `/stats` | A schema-validating consumer (e.g. a `jq` script) succeeds against a fresh snapshot. |
| `/metrics` | `promtool check metrics` against a fresh scrape succeeds; existing alert rules continue to match the same metric/label combinations. |
| `preferences.toml` | The GUI starts and the persisted theme is honoured. |
| Exit codes | `kill -TERM` and Ctrl-C both produce exit `0`; an invalid config produces exit `1`. |

If any of these fail on a same-major upgrade, that's a regression —
please open an issue.
