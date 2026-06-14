# Repository Instructions

## Commit And Push Gate

Before committing or pushing to GitHub, run the relevant local checks for the
files changed. Do not push code, packaging, or release workflow changes when
the corresponding local CI checks fail.

For code, scripts, packaging, resources, or Cargo changes, run the local
equivalents of the GitHub CI jobs before committing:

```sh
cargo +nightly fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/ci/check-icons.sh
cargo test --workspace --no-fail-fast
cargo build --workspace --release
```

For package/release changes, also build the affected package formats locally.
For a release, build every local release format before tagging or uploading:

```sh
./scripts/package-local.sh nix deb rpm flatpak
```

Include `archive` when the release should ship a tarball:

```sh
./scripts/package-local.sh nix archive deb rpm flatpak
```

For docs changes under `docs/`, run:

```sh
(cd docs && npm ci && npm run build)
```

For documentation-only changes outside the CI path, at minimum run:

```sh
git diff --check
```

If a check cannot be run locally because a required tool or service is missing,
record that explicitly before committing or pushing, and do not claim CI parity.

## Local-Only Release Process

Use local package builds for releases. Do not rely on GitHub Actions to create
release packages unless the user explicitly asks for that.

Release tags must point at the exact committed source used to build the
packages. Never upload packages built from a dirty working tree to a release
tag that points somewhere else.

Standard release flow:

1. Update the version in `Cargo.toml` and any packaging metadata that needs the
   new version.
2. Run the commit and push gate above, including every local package format that
   will be attached to the release.
3. Commit all release and packaging changes.
4. Push the release commit to `origin/main`.
5. Create an annotated tag on that exact commit:

   ```sh
   git tag -a vX.Y.Z -m vX.Y.Z
   git push origin vX.Y.Z
   ```

6. Build packages locally from a clean tree:

   ```sh
   git status --short
   ./scripts/package-local.sh nix deb rpm flatpak
   ```

   Include `archive` too when a tarball release artifact is wanted:

   ```sh
   ./scripts/package-local.sh nix archive deb rpm flatpak
   ```

7. Verify the local artifacts:

   ```sh
   find dist/packages -maxdepth 1 -type f -printf '%f %s bytes\n' | sort
   sha256sum dist/packages/*
   result-lumen/bin/lumen --version
   ```

8. Upload the locally built artifacts to the existing remote tag:

   ```sh
   ./scripts/upload-release-assets.sh vX.Y.Z
   ```

9. Verify the GitHub release:

   ```sh
   gh release view vX.Y.Z --json url,tagName,isDraft,isPrerelease,assets \
     --jq '{url, tagName, isDraft, isPrerelease, assets: [.assets[] | {name, size, state}]}'
   git ls-remote --tags origin vX.Y.Z
   ```

If a tag mismatch is discovered after uploading assets, commit and push the
source that produced the artifacts, move the tag to that commit both locally and
remotely, then verify the release again:

```sh
git tag -f vX.Y.Z <release-commit>
git push --force origin refs/tags/vX.Y.Z
git fetch --tags --force origin
git rev-parse vX.Y.Z
git ls-remote --tags origin vX.Y.Z
```

Only rewrite a published tag when the user has explicitly approved fixing that
release tag.
