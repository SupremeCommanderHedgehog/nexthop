# Contributing to nexthop

Thanks for your interest in contributing. nexthop is a single-maintainer
project today, but outside contributions are welcome — this document
describes the bar for getting changes merged.

For security vulnerabilities, **do not open a public issue** — follow the
private disclosure process in [SECURITY.md](SECURITY.md) instead.

---

## Dev setup

You need:

| Tool | Version |
|------|---------|
| Rust + Cargo | 1.77+ |
| Node.js | 18+ |
| npm | 9+ |
| Tauri CLI | v2 (installed via `npm install`) |

Platform prerequisites:

- **Windows** — WebView2 runtime (pre-installed on Windows 11; installer
  available from Microsoft).
- **macOS** — Xcode Command Line Tools (`xcode-select --install`).
- **Linux** — `libwebkit2gtk-4.1`, `libgtk-3`, and
  `libayatana-appindicator3` (or `libappindicator3`). Package names vary
  by distro.

## Building

See the [Building](README.md#building) section in the README for the
`npm install` / `npm run tauri dev` / `npm run tauri build` workflow.

## Tests must pass

Before opening a pull request, both test suites must be green:

```sh
cd src-tauri && cargo test --workspace
npm test
```

CI runs the same checks; PRs with red CI will not be reviewed.

## Fuzzing

The repo ships two libfuzzer harnesses under `src-tauri/fuzz/`:

- `config_parser` — feeds arbitrary bytes through `toml::from_str` →
  `RelayConfig::validate`, the highest panic-risk surface in the crate.
- `source_read` — exercises the UDP source's framing + oversize-drop
  arithmetic against arbitrary byte streams.

The dedicated [`Fuzz` workflow](.github/workflows/fuzz.yml) runs both
targets on every push to `master` (5 minutes each) and on a nightly
cron (30 minutes each). Crashes are uploaded as workflow artifacts.

Running locally requires nightly Rust and `cargo-fuzz`, and works on
Linux and macOS (libfuzzer's Windows support is patchy):

```sh
rustup install nightly
cargo install --locked cargo-fuzz
cd src-tauri/fuzz
cargo +nightly fuzz run config_parser
cargo +nightly fuzz run source_read
```

The fuzz crate is excluded from the main workspace, so it does not
affect `cargo build` / `cargo test` from the repo root and Windows
contributors can ignore it entirely.

## Branches and commits

- Branch off `master`. Name branches `feature/<slug>`, `fix/<slug>`, or
  `chore/<slug>`.
- Keep each commit focused. Rebase to clean up WIP commits before
  opening the PR.
- Commit message style: [Conventional Commits](https://www.conventionalcommits.org/).
  Subject is `<type>(<scope>): <summary>`, where `<type>` is one of
  `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
  `build`, `ci`, or `chore`. Body is free-form wrapped prose
  explaining *why*. Breaking changes use `!` after the type
  (e.g. `feat!:`) or a `BREAKING CHANGE:` footer.
  Rationale and trade-offs live in
  [ADR 0001](docs/adr/0001-conventional-commits-and-release-please.md);
  CI enforces the structural rules via `commitlint`.
- If your change closes a GitHub issue, add a `Closes #N` trailer.

## Signed commits required

Every commit merged into `master` must be cryptographically signed —
either a GPG signature (`git commit -S`) or an SSH signature
(`git config --global gpg.format ssh`). Unsigned commits will be
rejected at review.

To set up GPG signing once:

```sh
git config --global user.signingkey <YOUR_KEY_ID>
git config --global commit.gpgsign true
```

GitHub's docs cover this end-to-end:
<https://docs.github.com/en/authentication/managing-commit-signature-verification>.

The reason: nexthop is supply-chain-adjacent (it ships SBOMs and acts
as a network intermediary), and signed commits give the project a
verifiable chain of custody from author to release artifact.

## Code review

- The maintainer is the sole reviewer today.
- CI must be green.
- Expect feedback on scope creep — small, focused PRs land faster than
  large bundled ones.
- Allow a few days for review; ping the PR if it goes silent for more
  than a week.

## Reporting bugs vs vulnerabilities

- **Bug** — open a [GitHub issue](https://github.com/SupremeCommanderHedgehog/nexthop/issues)
  with reproduction steps, expected vs actual behavior, and the version
  (`nexthop --version`).
- **Vulnerability** — do **not** file a public issue. Follow the private
  disclosure process in [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed
under the project's [GPL-3.0-or-later license](LICENSE.md).
