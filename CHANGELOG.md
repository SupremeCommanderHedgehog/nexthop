# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While `nexthop` is pre-1.0, **minor** version bumps may include breaking changes;
**patch** version bumps are backwards-compatible only.

## [Unreleased]

## [0.2.2] - 2026-05-26

Patch release. Backwards-compatible — no config schema, CLI, or `/stats`
shape changes. Developer-tooling fix plus a documentation update.

### Changed
- Replaced the `nexthop@krypte.me` placeholder with **Patrick S Connallon**
  in every `// Copyright (C)` and `// Architect:` header across the Rust
  crate, and in the JSX footer in `src/components/ConfigTab.tsx`. 19 files
  touched; comment-only, no runtime impact. (#15)

### Fixed
- `scripts/bump-version.ps1` no longer pulls in semver-patch transitive
  dependency updates as a side effect of the bump. Switched from
  `cargo update -p nexthop` (which re-resolves transitives) to
  `cargo metadata --format-version 1 --quiet`, which rewrites
  `Cargo.lock` only to match the workspace manifests. Verified on the
  0.2.2 cut — `Cargo.lock` diff was exactly the workspace member version
  line. (#14)

## [0.2.1] - 2026-05-26

Patch release. Backwards-compatible only — no config schema changes,
no CLI changes, no `/stats` shape changes. Bundles security dependency
bumps and tooling/documentation introduced after 0.2.0.

### Added
- `SECURITY.md` — coordinated-disclosure security policy pointing at GitHub
  private vulnerability reporting.
- Gitleaks pre-commit hook (`.pre-commit-config.yaml`) and GitHub Actions
  workflow (`.github/workflows/gitleaks.yml`) for secret scanning.
- `CHANGELOG.md` (this file).
- `scripts/bump-version.ps1` — single-script bump for `package.json`,
  `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, plus `Cargo.lock`
  and `package-lock.json` refresh.

### Changed
- Bumped `vite` from `^5.4.10` to `^6.4.2` (major). Required to pick up the
  path-traversal fix; no v5.x patch was released.
- `Cargo.lock` refreshed during the version bump — picked up incidental
  semver-patch updates of several transitive deps
  (`filetime`, `hashbrown`, `kqueue-sys`, `libredox`, plus new transitive
  `bs58`). No direct-dependency or feature changes.

### Fixed
- Gitleaks CI now downloads the release tarball with a pinned sha256
  verification instead of the prior install path.

### Security
- Pinned `esbuild` to `^0.25.0` via npm `overrides` —
  [GHSA-67mh-4wv8-2f99](https://github.com/advisories/GHSA-67mh-4wv8-2f99)
  (dev-server CORS).
- Pinned `ws` to `^8.20.1` via npm `overrides` —
  [GHSA-58qx-3vcg-4xpx](https://github.com/advisories/GHSA-58qx-3vcg-4xpx)
  (uninitialized memory disclosure).
- Bumped `vite` to `^6.4.2` (above) —
  [GHSA-4w7w-66w2-5vf9](https://github.com/advisories/GHSA-4w7w-66w2-5vf9)
  (path traversal in optimized-deps `.map` handling).
- The `glib < 0.20.0` advisory
  ([GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g))
  remains open: it is a Linux-only transitive via `tauri → wry → gtk-rs 0.18`
  and there is no `tauri 2.x` release on `gtk-rs 0.20` yet.

## [0.2.0] - 2026-05-08

Initial release. See the
[v0.2.0 release notes](https://github.com/SupremeCommanderHedgehog/nexthop/releases/tag/v0.2.0)
and feature-group issues [#1](https://github.com/SupremeCommanderHedgehog/nexthop/issues/1)–[#8](https://github.com/SupremeCommanderHedgehog/nexthop/issues/8).

### Added
- Core relay engine — TCP/UDP cross-protocol forwarding with fan-out to
  multiple destinations.
- Multicast and broadcast support for IPv4 and IPv6, with configurable
  interface and TTL.
- Per-destination back-pressure: `drop_newest` (default) and `block`
  overflow policies, each with an independent queue.
- Token-bucket rate limiting (`bytes_per_second` + `burst_size`) on the
  source read path.
- Live config hot-reload — `[rate_limit]` changes take effect on the next
  packet without restarting any tasks.
- Optional HTTP health/stats endpoint (`/health`, `/stats`) with JSON
  counter snapshots per endpoint.
- Tauri v2 desktop GUI with Configuration, Monitoring, and Preferences
  tabs (dark/light theme, persisted to `preferences.toml`).
- Headless mode (`--no_gui`) and structured JSON logs (`--log-format json`)
  for service / CI deployments.
- Unit test suite — 84 tests across 9 modules
  (`error`, `stats`, `rate_limiter`, `config`, `prefs`, `transport`, `relay`,
  `gui/monitor_page`, `gui/config_page`).

[Unreleased]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SupremeCommanderHedgehog/nexthop/releases/tag/v0.2.0
