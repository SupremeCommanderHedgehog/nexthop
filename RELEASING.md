# Releasing nexthop

The release flow is driven by
[release-please](https://github.com/googleapis/release-please) — see
[ADR 0001](docs/adr/0001-conventional-commits-and-release-please.md)
for the rationale and trade-offs.

This document is the operational manual. If you are about to cut a
release, follow the steps in
[**Standard release**](#standard-release). If something has gone
sideways and you need to bump versions out-of-band, follow
[**Emergency release**](#emergency-release).

---

## Standard release

### 1. Wait for the release PR

Every time a `feat:`, `fix:`, or `BREAKING CHANGE` commit lands on
`master`, the `release-please` workflow (`.github/workflows/release-please.yml`)
updates (or opens, if it does not yet exist) a single release PR with:

- a new `CHANGELOG.md` section assembled from the Conventional
  Commit subjects since the last tag,
- version bumps in `package.json`, `package-lock.json`,
  `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`,
- a new entry in `.release-please-manifest.json`.

The PR is titled `chore(main): release <version>`. The version is
inferred from commit types (`feat:` → minor, `fix:` → patch,
`BREAKING CHANGE` → minor while < 1.0, major after).

### 2. Sign the tip commit

The release PR is pushed by the GitHub Actions bot. **The bot commit
is web-flow signed, not GPG-signed by the maintainer key.** Master
branch protection requires signed commits, so the PR will not merge
as-is.

Replace the bot's tip commit with a signed equivalent:

```sh
gh pr checkout <pr-number>
git commit -S --amend --no-edit
git push --force-with-lease
```

`--amend --no-edit` preserves the bot's message and file contents
verbatim; only the commit object's signature changes. Wait for CI to
re-run on the new SHA.

### 3. Review the diff

- `CHANGELOG.md` — does the auto-assembled section match what you
  expect? Reorder or rewrite freely if a commit subject reads badly in
  release-note context.
- Version files — sanity-check that all four (`package.json`,
  `package-lock.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json`) moved to the same number.
- Any other diff lines — release-please occasionally touches its own
  manifest; nothing else should appear.

### 4. Merge

Use the **Merge** (not squash, not rebase) strategy to preserve the
signed tip commit. release-please will reach into `master`'s new
state on its next run, see the tag it just produced, and start a
fresh release PR for the next cycle.

### 5. Publish the GitHub release

release-please tags the merge commit as `v<version>` and creates a
draft GitHub release with the CHANGELOG section as the body. Either:

- Open the draft release in the GitHub UI and click **Publish**, or
- `gh release edit v<version> --draft=false`.

The existing `release.yml` build matrix will pick up the published
tag and attach platform binaries + SBOMs the same way it did before
release-please.

---

## Emergency release

If release-please is broken, GitHub Actions is down, or you need to
ship a fix without waiting for the bot, the manual flow from before
the adoption still works:

```sh
./scripts/bump-version.ps1 <new-version>
# Edit CHANGELOG.md by hand: move [Unreleased] entries into a new
# [<new-version>] - <YYYY-MM-DD> section.
git -c user.email="…" -c user.signingkey=… add -A
git commit -S -m "chore(release): <new-version>"
git tag -s v<new-version> -m "Release <new-version>"
git push origin master --tags
gh release create v<new-version> --notes-file <(awk '…')
```

After an emergency release:

- Update `.release-please-manifest.json` so release-please's next run
  picks up the new baseline rather than trying to re-bump the same
  version. The line is `{".": "<new-version>"}`.
- Commit that change with a `chore(release-please): sync manifest to
  <new-version>` subject so release-please sees it as a pure manifest
  update.

---

## Adding a new version-tracked file

If a future change introduces another file that needs to track the
project version (a Tauri sidecar binary's `package.json`, a new
crate, etc.):

1. Add the path to `release-please-config.json#packages.".".extra-files`,
   either as a string (if the file is a JSON document with a
   `$.version` field) or as an object with explicit `type` + `path` +
   `jsonpath`. For Cargo.toml-shaped files, use the
   `# x-release-please-version` annotation pattern instead.
2. Update `scripts/bump-version.ps1` to handle the same file so the
   emergency path stays in sync.
3. Mention it in this document under
   [Emergency release](#emergency-release) so a maintainer running
   the manual flow updates it.
