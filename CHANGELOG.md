# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While `nexthop` is pre-1.0, **minor** version bumps may include breaking changes;
**patch** version bumps are backwards-compatible only.

## [0.6.2](https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.6.1...v0.6.2) (2026-07-13)


### Bug Fixes

* **frontend:** make relay-status poll updater pure (App.tsx) ([1e101e7](https://github.com/SupremeCommanderHedgehog/nexthop/commit/1e101e75d9eb0b6462f2c7d9157477e76464ede9))
* **frontend:** make relay-status poll updater pure (App.tsx) ([aebd184](https://github.com/SupremeCommanderHedgehog/nexthop/commit/aebd184a28e8e84909255777bcc80c0b06081d33)), closes [#150](https://github.com/SupremeCommanderHedgehog/nexthop/issues/150)
* **relay:** eliminate status races with run-id-tagged events ([86bd2a2](https://github.com/SupremeCommanderHedgehog/nexthop/commit/86bd2a2b5b9e789d3c0125f87123a0fdba06d9cf))
* **relay:** eliminate status races with run-id-tagged events ([8ad4337](https://github.com/SupremeCommanderHedgehog/nexthop/commit/8ad4337621261236a1c6d9b2e3768cc2bc8d4298)), closes [#155](https://github.com/SupremeCommanderHedgehog/nexthop/issues/155)

## [0.6.1](https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.6.0...v0.6.1) (2026-07-13)


### Bug Fixes

* **frontend:** add src/vite-env.d.ts for vite/client ambient types ([98d99a7](https://github.com/SupremeCommanderHedgehog/nexthop/commit/98d99a76d5696806e53dcfa0486a2ee8e199075f))
* **frontend:** add src/vite-env.d.ts for vite/client ambient types ([eced04d](https://github.com/SupremeCommanderHedgehog/nexthop/commit/eced04dad9613056c5927b6990b002da32fe3f71)), closes [#146](https://github.com/SupremeCommanderHedgehog/nexthop/issues/146)

## [0.6.0](https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.5.0...v0.6.0) (2026-07-13)


### Features

* adopt Conventional Commits and release-please ([c18f9d8](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c18f9d85734499b662fc9291b9ab9eff9532d652))
* adopt Conventional Commits and release-please ([7c4fe53](https://github.com/SupremeCommanderHedgehog/nexthop/commit/7c4fe53b360844f2ad89507c36058cf59fa9b655))
* byte_swap_16 and byte_swap_32 transforms ([a47a704](https://github.com/SupremeCommanderHedgehog/nexthop/commit/a47a704f363b689e3bac2e490f615d73f8d7f1dd))
* byte_swap_16 and byte_swap_32 transforms ([7f94879](https://github.com/SupremeCommanderHedgehog/nexthop/commit/7f94879af6e03f95232296de7ad6891df0c8c564)), closes [#81](https://github.com/SupremeCommanderHedgehog/nexthop/issues/81)
* drop_larger_than transform ([8b5126a](https://github.com/SupremeCommanderHedgehog/nexthop/commit/8b5126a4671bf17eb30b54c33e23d36d6f2f9de4))
* drop_larger_than transform ([ab4702e](https://github.com/SupremeCommanderHedgehog/nexthop/commit/ab4702ecd4d48efea6a6a7c11c13ad27397b2427)), closes [#80](https://github.com/SupremeCommanderHedgehog/nexthop/issues/80)
* per-destination transform pipeline with drop_smaller_than ([c3ccdd1](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c3ccdd1e8c882d98a6631386b26787870db3b8e7))
* per-destination transform pipeline with drop_smaller_than ([2868340](https://github.com/SupremeCommanderHedgehog/nexthop/commit/286834006e529d2d02a73c6987492787d1ad2638)), closes [#79](https://github.com/SupremeCommanderHedgehog/nexthop/issues/79)
* prepend_timestamp transform ([b2abc9e](https://github.com/SupremeCommanderHedgehog/nexthop/commit/b2abc9e05fc6efe5105414867ad5938cf25daa73))
* prepend_timestamp transform ([d695037](https://github.com/SupremeCommanderHedgehog/nexthop/commit/d695037b1a1498628af9f4213db5093234f28055)), closes [#82](https://github.com/SupremeCommanderHedgehog/nexthop/issues/82)
* regex_filter transform ([42a274f](https://github.com/SupremeCommanderHedgehog/nexthop/commit/42a274f3eeb795bb8ae49820bbe132c53782f90a))
* regex_filter transform ([b09ae03](https://github.com/SupremeCommanderHedgehog/nexthop/commit/b09ae03d25667b88baddf0281f669181c31f02d6)), closes [#83](https://github.com/SupremeCommanderHedgehog/nexthop/issues/83)


### Bug Fixes

* accept IPv6 multicast addresses in the GUI validator ([c28ddfb](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c28ddfba9af47fb8ec01028e919b0e8afd131a03))
* accept IPv6 multicast addresses in the GUI validator ([d4728d8](https://github.com/SupremeCommanderHedgehog/nexthop/commit/d4728d897fce85bd1c0a4f86ffc2968d35948318)), closes [#85](https://github.com/SupremeCommanderHedgehog/nexthop/issues/85)
* **deps:** bump plist to pull quick-xml 0.41 (RUSTSEC-2026-0194/0195) ([#135](https://github.com/SupremeCommanderHedgehog/nexthop/issues/135)) ([a05cdc5](https://github.com/SupremeCommanderHedgehog/nexthop/commit/a05cdc5a05ef1a6b672bf0139afabf6a0238d0ce)), closes [#134](https://github.com/SupremeCommanderHedgehog/nexthop/issues/134)
* keep gui feature enabled through tauri dev / build ([d8f6942](https://github.com/SupremeCommanderHedgehog/nexthop/commit/d8f6942f8ff7030ca14b77116c7dce26285b2f02))
* keep gui feature enabled through tauri dev / build ([ec44317](https://github.com/SupremeCommanderHedgehog/nexthop/commit/ec4431714c9fa5a1862743a32c05752aaac2e6be))
* pre-populate broadcast addresses for broadcast destinations ([af26839](https://github.com/SupremeCommanderHedgehog/nexthop/commit/af26839728d5c237ab0129a124db47737843851f))
* pre-populate broadcast addresses for broadcast destinations ([584b123](https://github.com/SupremeCommanderHedgehog/nexthop/commit/584b123e54979bc8ee9074252fcd508b93f9cfa6)), closes [#74](https://github.com/SupremeCommanderHedgehog/nexthop/issues/74)
* red-border non-host-shaped strings in unicast host inputs ([dd43799](https://github.com/SupremeCommanderHedgehog/nexthop/commit/dd43799bfbf4c47244ac14802420d36ce66392fe))
* red-border non-host-shaped strings in unicast host inputs ([72050dc](https://github.com/SupremeCommanderHedgehog/nexthop/commit/72050dc1c9baa2f50e93fdbb5948386c1d88e24b)), closes [#86](https://github.com/SupremeCommanderHedgehog/nexthop/issues/86)
* relabel multicast Mode dropdown and default source to subscriber ([09168fd](https://github.com/SupremeCommanderHedgehog/nexthop/commit/09168fd01033f9c8c973d31294476d4fd480d858))
* relabel multicast Mode dropdown and default source to subscriber ([7a37dfd](https://github.com/SupremeCommanderHedgehog/nexthop/commit/7a37dfdef006818da967044376d13740ba703684)), closes [#75](https://github.com/SupremeCommanderHedgehog/nexthop/issues/75) [#76](https://github.com/SupremeCommanderHedgehog/nexthop/issues/76)
* reset address to a sensible default when cast_mode changes ([513c401](https://github.com/SupremeCommanderHedgehog/nexthop/commit/513c40186fb7704dbb79e001151ae2f7e5bf6338))
* reset address to a sensible default when cast_mode changes ([41cf970](https://github.com/SupremeCommanderHedgehog/nexthop/commit/41cf9704e51776d3f240da62ebb95b3f1c911bef)), closes [#84](https://github.com/SupremeCommanderHedgehog/nexthop/issues/84)

## [0.5.0](https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.4.0...v0.5.0) (2026-05-28)


### Features

* byte_swap_16 and byte_swap_32 transforms ([266f66b](https://github.com/SupremeCommanderHedgehog/nexthop/commit/266f66b8b697d2cb0efad0356ef68e8227fe06b0))
* byte_swap_16 and byte_swap_32 transforms ([afeeb24](https://github.com/SupremeCommanderHedgehog/nexthop/commit/afeeb24e79be5720bcbd122dac38d67c0c73c0db)), closes [#81](https://github.com/SupremeCommanderHedgehog/nexthop/issues/81)
* drop_larger_than transform ([852d187](https://github.com/SupremeCommanderHedgehog/nexthop/commit/852d187bd9251ba53645b46aad729e77c5dff1e6))
* drop_larger_than transform ([a0f682b](https://github.com/SupremeCommanderHedgehog/nexthop/commit/a0f682b2bb03ee923bcff45bc22ed4fbcc9c7d61)), closes [#80](https://github.com/SupremeCommanderHedgehog/nexthop/issues/80)
* per-destination transform pipeline with drop_smaller_than ([1b6c56a](https://github.com/SupremeCommanderHedgehog/nexthop/commit/1b6c56a78b6840c82e082e045e2d38b4a126a750))
* per-destination transform pipeline with drop_smaller_than ([c7e8ea1](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c7e8ea19ac3d4cb1a655c02e82e10cb80f1242aa)), closes [#79](https://github.com/SupremeCommanderHedgehog/nexthop/issues/79)
* prepend_timestamp transform ([046ad81](https://github.com/SupremeCommanderHedgehog/nexthop/commit/046ad81d28b3870d4609d7c65dc548ff45364ca2))
* prepend_timestamp transform ([3174a4b](https://github.com/SupremeCommanderHedgehog/nexthop/commit/3174a4b32336ca2a4b7b05df0ea79c6b0288515a)), closes [#82](https://github.com/SupremeCommanderHedgehog/nexthop/issues/82)
* regex_filter transform ([9152b93](https://github.com/SupremeCommanderHedgehog/nexthop/commit/9152b933292128c67de9e345832b863e296602aa))
* regex_filter transform ([17e6766](https://github.com/SupremeCommanderHedgehog/nexthop/commit/17e6766ee3c69d464fc2f81a25a2d2ca46c1cc65)), closes [#83](https://github.com/SupremeCommanderHedgehog/nexthop/issues/83)


### Bug Fixes

* accept IPv6 multicast addresses in the GUI validator ([0c0084a](https://github.com/SupremeCommanderHedgehog/nexthop/commit/0c0084a3a268ff57db7c9d488d1daa6334cd429c))
* accept IPv6 multicast addresses in the GUI validator ([c13d34e](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c13d34e282fad5e4f7a4b3bbe98cf23a56df0602)), closes [#85](https://github.com/SupremeCommanderHedgehog/nexthop/issues/85)
* keep gui feature enabled through tauri dev / build ([7bc683d](https://github.com/SupremeCommanderHedgehog/nexthop/commit/7bc683daa6ecf28bcdda628de2c87890d8f3772d))
* keep gui feature enabled through tauri dev / build ([f83f143](https://github.com/SupremeCommanderHedgehog/nexthop/commit/f83f14305c6849cc9376d058d1b63710f014d887))
* pre-populate broadcast addresses for broadcast destinations ([bd255f7](https://github.com/SupremeCommanderHedgehog/nexthop/commit/bd255f7d103a92cacd392cbf0754db41cf944c11))
* pre-populate broadcast addresses for broadcast destinations ([5298ebd](https://github.com/SupremeCommanderHedgehog/nexthop/commit/5298ebde39806501c80dd64047c4420586cbd8b2)), closes [#74](https://github.com/SupremeCommanderHedgehog/nexthop/issues/74)
* red-border non-host-shaped strings in unicast host inputs ([79f2b7c](https://github.com/SupremeCommanderHedgehog/nexthop/commit/79f2b7c6b62c1e8b8454c064d2eb2e04e5c9d6f3))
* red-border non-host-shaped strings in unicast host inputs ([2639956](https://github.com/SupremeCommanderHedgehog/nexthop/commit/263995623b93f2168c213949456df2230cea54d3)), closes [#86](https://github.com/SupremeCommanderHedgehog/nexthop/issues/86)
* relabel multicast Mode dropdown and default source to subscriber ([3a73bb8](https://github.com/SupremeCommanderHedgehog/nexthop/commit/3a73bb8cd7ff4e8b31f2c6f5cb3edd2bbf8201b7))
* relabel multicast Mode dropdown and default source to subscriber ([c240fa3](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c240fa30131dd4f484cabe768f8b22120ff5c518)), closes [#75](https://github.com/SupremeCommanderHedgehog/nexthop/issues/75) [#76](https://github.com/SupremeCommanderHedgehog/nexthop/issues/76)
* reset address to a sensible default when cast_mode changes ([3cbb9b5](https://github.com/SupremeCommanderHedgehog/nexthop/commit/3cbb9b55d70513c019209461162423b9b63d89a2))
* reset address to a sensible default when cast_mode changes ([81c8667](https://github.com/SupremeCommanderHedgehog/nexthop/commit/81c8667847c8285ea7e5e291cb1ad954ed30af74)), closes [#84](https://github.com/SupremeCommanderHedgehog/nexthop/issues/84)

## [0.4.0](https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.3.0...v0.4.0) (2026-05-28)


### Features

* adopt Conventional Commits and release-please ([3060daf](https://github.com/SupremeCommanderHedgehog/nexthop/commit/3060daf0f9ed57706872808036a4796cfb4b05b8))
* adopt Conventional Commits and release-please ([c7a76db](https://github.com/SupremeCommanderHedgehog/nexthop/commit/c7a76dba0ce7b3dd48c77e1c7a153d3460b3d9e5))

## [Unreleased]

## [0.3.0] - 2026-05-27

First minor release after the 0.2.x line. Pre-1.0 minor bump justified by
a new public endpoint (`/metrics`), an additive `/stats` JSON shape
change, and a counter-semantics shift documented below.

### Added
- **Prometheus `/metrics` endpoint** alongside `/health` and `/stats` on
  the existing `health_port`. Eight metrics per endpoint (`rx_bytes_total`,
  `tx_bytes_total`, `messages_total`, `errors_total`, `dropped_total`,
  `connections_opened_total`, `active_connections`, `uptime_seconds`),
  all labeled with `endpoint="<display name>"`. Native scrape-config in
  MANUAL. No new dependency — rendered directly from existing snapshots.
  (#20)
- **Per-reason `dropped` breakdown**: four new sub-counters
  (`dropped_overflow`, `dropped_oversize`, `dropped_validation`,
  `dropped_write_error`) surfaced in `/stats` JSON, `/metrics`, and the
  GUI Monitor tab. The existing `dropped` field stays as the sum, so
  consumers of the pre-breakdown shape see no break. (#32)
- **Rust CI workflow** (`.github/workflows/rust.yml`) — `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo build` + `cargo test --workspace` on
  ubuntu / windows / macos. Single `rust-ci-success` aggregator status
  context. (#17)
- **`cargo-audit` + `cargo-deny`** in CI with a new `deny.toml` covering
  advisories, licenses, bans, and source registries. The unmaintained
  gtk-rs 0.18 family + glib unsoundness + unic-* are allowlisted with
  reasons. (#19)
- **Branch protection on master** — signed commits required, `rust-ci-success`
  + `gitleaks` status checks required, no force-push, no deletions,
  `enforce_admins: true`. (#18)
- **Release workflow on tag push** (`.github/workflows/release.yml`) —
  drives `tauri-apps/tauri-action@v0` across linux-x86_64, windows-x86_64,
  and macos-universal. Bundles `.AppImage` / `.deb` / `.rpm` / `.msi` /
  `.dmg` / `.app.tar.gz`. (#21)
- **SBOMs in release artifacts** — SPDX 2.3 JSON and CycloneDX 1.5 JSON
  generated by `anchore/syft` and attached to each tagged release. (#29)
- **GitHub Private Vulnerability Reporting** enabled (previously gated on
  the repo being public).

### Changed
- **Repository visibility flipped to public** to unblock the GitHub-Pro
  paywall on branch protection. Aligns with the GPL-3.0-or-later license
  and activates `SECURITY.md`'s `Report a vulnerability` button.
- **Counter semantics** (#32): UDP source oversize datagrams now increment
  `dropped_oversize` instead of `errors` (it's a content-policy drop, not
  a network error). TCP/UDP destination write errors now dual-count
  `errors + dropped_write_error` (the write failed and the packet was
  lost — both dimensions). Operators dashboarding on the old counters
  will see values shift accordingly.
- **`src-tauri/Cargo.toml`** now declares `license = "GPL-3.0-or-later"`
  in the `[package]` table. The SPDX comment header already documented
  the license; this lets cargo-deny and the crates.io ecosystem read it.

### Fixed
- Six pre-existing clippy / rustfmt issues in `src-tauri/src/relay.rs`
  cleaned up to make `cargo clippy -D warnings` a viable CI gate:
  modernized `std::io::Error::other` (×6), `while let` over a `loop +
  match` (×1), and a structural lint suppression where moving items
  around a test module would churn ~400 lines of file order for no
  semantic benefit.

### Removed
- Six vestigial Rust source files at `src/{config,error,rate_limiter,relay,stats,transport}.rs`.
  They were stale copies of the active sources under `src-tauri/src/` and
  were never compiled (the workspace member is in `src-tauri`). Net
  removal of ~2300 lines. (#22)

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

[Unreleased]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/SupremeCommanderHedgehog/nexthop/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SupremeCommanderHedgehog/nexthop/releases/tag/v0.2.0
