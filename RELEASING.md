# Releasing

The workspace publishes four crates to crates.io, always with the same version:

| Crate               | Depends on                                |
|---------------------|-------------------------------------------|
| `dbmigrator_core`   | —                                         |
| `dbmigrator_macros` | `dbmigrator_core`                         |
| `dbmigrator`        | `dbmigrator_core`, `dbmigrator_macros`    |
| `dbmigrator_cli`    | `dbmigrator`                              |

## Prerequisites

- You are an owner of all four crates on crates.io (`cargo owner --list <crate>`; an existing
  owner adds you with `cargo owner --add <github-login> <crate>`).
- `cargo login` with a crates.io API token that has the `publish-update` scope (and
  `publish-new` if a crate is published for the first time).
- Push access to `github.com/dbmigrator/dbmigrator`.

## Steps

1. Start from an up-to-date, clean `main`:
   ```bash
   git checkout main && git pull --ff-only && git status
   ```
2. Set the new version in all four `Cargo.toml` files — both `version` under `[package]` and
   the `version` of every internal dependency (`dbmigrator_core`, `dbmigrator_macros`,
   `dbmigrator`). Pre-releases use a `-alpha` suffix, e.g. `0.4.7-alpha`.
   ```bash
   grep -n 'version = "' */Cargo.toml   # every internal entry must show the new version
   ```
3. Test and check that every crate packages and builds on its own:
   ```bash
   cargo test --workspace
   cargo publish --dry-run --workspace
   ```
   The dry run builds each crate in isolation, outside the workspace's feature unification —
   it is what catches missing dependency features or missing crates.io metadata
   (`description`, `license`).
4. Commit, tag and push:
   ```bash
   git commit -am "Bump version X.Y.Z"
   git tag -a vX.Y.Z -m "vX.Y.Z"
   git push origin main vX.Y.Z
   ```
5. Publish in dependency order:
   ```bash
   cargo publish -p dbmigrator_core
   cargo publish -p dbmigrator_macros
   cargo publish -p dbmigrator
   cargo publish -p dbmigrator_cli
   ```
   A published version can never be overwritten or deleted, only yanked
   (`cargo yank --version X.Y.Z <crate>`); fixes go out as a new version.
6. Check the release: the crates.io pages of all four crates show the new version, docs.rs has
   built `dbmigrator`, and `cargo install dbmigrator_cli --version X.Y.Z` works.
7. Open the next development version: set all four crates (and their internal dependencies) to
   the next pre-release, e.g. `0.4.7-alpha`, and commit it as `Bump version 0.4.7-alpha`.

## Known issues

- With a pre-release version, `cargo publish --dry-run --workspace` has been seen to fail on
  `dbmigrator_cli` with `no hash listed for dbmigrator vX.Y.Z-alpha`. The other three crates
  are verified normally; verify the CLI with `cargo publish -p dbmigrator_cli --dry-run` after
  `dbmigrator` is published.
