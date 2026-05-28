# ADR 0001 — Adopt Conventional Commits and release-please

- **Status:** accepted
- **Date:** 2026-05-28
- **Closes:** [#34](https://github.com/SupremeCommanderHedgehog/nexthop/issues/34)

## Context

Up to 0.3.0 the release flow was entirely manual:

1. Run `scripts/bump-version.ps1 <new-version>` to update three version
   files and refresh both lockfiles.
2. Hand-edit `CHANGELOG.md` — move the `[Unreleased]` entries into a
   new `[<version>] - <date>` section.
3. Stage everything, create a signed commit (`git commit -S -m "Release
   <version>"`).
4. Create a signed annotated tag (`git tag -s v<version> -m "Release
   <version>"`).
5. `git push origin master --tags`.
6. `gh release create v<version> --notes-file …` to publish the GitHub
   release.

The flow is reproducible — `bump-version.ps1` and `RELEASING.md` would
keep working forever — but it has two systemic problems:

- **Changelog drift.** `[Unreleased]` is curated manually as PRs land.
  An entry can be forgotten, mislabelled (`Added` vs `Changed`), or
  attributed to the wrong PR number. The fix is reactive (notice during
  the bump, dig through `git log`, patch the section).
- **Implicit versioning rules.** "Should this be a minor or a patch?"
  is answered by the maintainer reading every PR diff at release time.
  The signal exists in the PRs themselves (`feat:` vs `fix:` vs
  `BREAKING CHANGE`) but the flow has no place to record or read it.

Both problems compound as the contributor count grows. At a single-
maintainer scale they are an annoying ritual; at three contributors
they will be incident-prone.

## Decision

**Adopt** [Conventional Commits](https://www.conventionalcommits.org/)
and [release-please](https://github.com/googleapis/release-please).

Concretely:

- Every commit on `master` (i.e. every commit that lands through a PR)
  uses Conventional Commits format. Subject: `<type>(<scope>): <summary>`.
  Body: free-form wrapped prose. Breaking changes either use `!` after
  the type (`feat!:`) or include a `BREAKING CHANGE:` footer.
- `.commitlintrc.json` + a `commitlint` GitHub workflow validate every
  commit in a PR. Structural rules (type, scope shape, breaking-change
  marker) are enforced; subject case and body line-length are relaxed
  so we keep our existing wrapped-prose style.
- `release-please-config.json` + `.release-please-manifest.json` +
  `.github/workflows/release-please.yml` keep a single, always-up-to-
  date release PR on `master`. Merging that PR cuts the version, ships
  the CHANGELOG, and creates the tag.

The hand-rolled `scripts/bump-version.ps1` stays as the fallback for
out-of-band bumps (e.g. emergency security releases pushed straight to
`master` without the bot). It is no longer the primary path.

## Known trade-offs

### Bot commits are not GPG-signed

The release-please action pushes its branch and commits using the
default `GITHUB_TOKEN`. The resulting commits are **web-flow signed**
by GitHub, not GPG-signed with the maintainer key. This clashes with
the global "every commit on this machine is GPG-signed" rule
documented in the maintainer's `~/.claude/CLAUDE.md`, and with master
branch protection's "require signed commits" requirement.

The mitigation, documented in `RELEASING.md`, is that the release flow
on the maintainer side is a two-step dance:

1. Wait for release-please to open or update the release PR.
2. Check the PR out locally, amend the tip commit with a GPG-signed
   replacement (`git commit -S --amend --no-edit` then force-push to
   the release branch), and merge.

The amend keeps the bot's CHANGELOG and version-file edits intact and
just re-signs the commit object. It costs one extra command per
release and preserves the branch-protection guarantee that everything
landing on `master` is signed by the maintainer.

### Two-version-file fan-out

The release-please config has explicit entries for `package.json`,
`package-lock.json` (top-level `$.version` *and* the inner
`packages.[""].version`), `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` (via an `# x-release-please-version` marker on
the version line). Adding a new manifest later means extending
`release-please-config.json#packages.".".extra-files`.

This is the most likely future-breakage surface — npm and Cargo
lockfile layouts have evolved before — so the fallback bump script
stays as a recovery path.

### Strictly Conventional Commits

Free-form commit subjects ("Add per-destination rate limiting",
"Hot-reload destinations: add/remove/identity-change via supervisor")
are gone. Future subjects look like `feat(relay): per-destination rate
limiting` or `refactor(relay): supervisor-driven hot-reload`. The
historical commits before this ADR keep their free-form form — no
rewrite of history. release-please only parses commits from the most
recent tag onward, so the prior subjects are inert.

## Alternatives considered

- **Decline, write `RELEASING.md`.** Keeps the GPG-signing rule
  intact with no extra steps. Rejected because the changelog-drift
  and implicit-versioning problems are real and getting worse, and
  the bot-signing dance is a one-line `--amend` per release rather
  than ongoing friction.
- **Squash-merge with PR-title-as-conventional-commit.** Would let us
  enforce CC only on PR titles via something like
  `amannn/action-semantic-pull-request` and skip the per-commit
  commitlint setup. Rejected because this project uses merge-commit
  preserves the body of every individual commit (which is where the
  reasoning lives, especially during long refactor branches), and
  squashing throws that away.
- **Custom shell-script release flow with Conventional Commits parsing
  in-house.** A weekend project to write, an ongoing maintenance burden
  to keep working against `git log --grep` edge cases. release-please
  is the upstream tool that already does this.

## Consequences

- Every new contributor needs to read CONTRIBUTING.md's commit
  conventions section and follow Conventional Commits. The
  `commitlint` workflow surfaces violations on PRs.
- The maintainer's release-day work shrinks to: amend the release PR
  tip commit with a signed commit, merge, run `gh release create`.
- The `scripts/bump-version.ps1` script remains, but is now the
  emergency-bypass path, not the primary one. `RELEASING.md`
  documents both.
- New ADRs go under `docs/adr/0NNN-<slug>.md`. This ADR establishes
  that directory.
