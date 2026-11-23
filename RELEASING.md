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

```bash
./release.sh X.Y.Z X.Y.(Z+1)-alpha     # e.g. ./release.sh 0.5.0 0.5.1-alpha
```

The script runs on an up-to-date `main` and:

1. sets the version in all four `Cargo.toml` files (`[package]` and internal dependencies),
2. runs `cargo test --workspace` and `cargo publish --dry-run --workspace` — the dry run builds
   each crate in isolation, which catches missing dependency features or crates.io metadata,
3. commits `Bump version X.Y.Z` and tags `vX.Y.Z`,
4. asks for confirmation, pushes `main` and the tag, and publishes the crates in dependency order,
5. opens the next development version and pushes `Bump version X.Y.(Z+1)-alpha`.

A published version can never be overwritten or deleted, only yanked
(`cargo yank --version X.Y.Z <crate>`); fixes go out as a new version. If publishing stops
halfway, publish the remaining crates with `cargo publish -p <crate>` and finish step 5 by hand.

Afterwards check the release: the crates.io pages of all four crates show the new version,
docs.rs has built `dbmigrator`, and `cargo install dbmigrator_cli --version X.Y.Z` works.

## Known issues

- With a pre-release version, the dry run (`cargo publish --dry-run --workspace`) has been seen to fail on
  `dbmigrator_cli` with `no hash listed for dbmigrator vX.Y.Z-alpha`. The other three crates
  are verified normally; verify the CLI with `cargo publish -p dbmigrator_cli --dry-run` after
  `dbmigrator` is published.
