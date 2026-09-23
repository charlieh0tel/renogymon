# Releasing

All crates share one version: `[workspace.package].version` in the
root `Cargo.toml`.

1. On a clean `main`, level with `origin/main`, run the checks:
   `cargo +nightly fmt --check`, `cargo clippy --workspace
   --all-targets`, `cargo test --workspace`, and the gated system
   tests:
   `cargo test -p renogymon-archiver --test system -- --ignored`,
   `cargo test -p renogymon-archiver-puller --test pull -- --ignored`.
2. Bump the version in the root `Cargo.toml`; run `cargo check
   --workspace` to update `Cargo.lock`.
3. `git commit -m "release: vX.Y.Z" Cargo.toml Cargo.lock`
4. `git tag vX.Y.Z && git push origin main vX.Y.Z`

Pushing the tag makes CI (`.github/workflows/build-deb.yml`) build the
`.deb`s and publish the GitHub Release.  Do not build `.deb`s locally.
