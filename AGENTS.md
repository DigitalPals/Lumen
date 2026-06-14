# Repository Instructions

## Local-Only Release Process

Use local package builds for releases. Do not rely on GitHub Actions to create
release packages unless the user explicitly asks for that.

Release tags must point at the exact committed source used to build the
packages. Never upload packages built from a dirty working tree to a release
tag that points somewhere else.

Standard release flow:

1. Update the version in `Cargo.toml` and any packaging metadata that needs the
   new version.
2. Commit all release and packaging changes.
3. Push the release commit to `origin/main`.
4. Create an annotated tag on that exact commit:

   ```sh
   git tag -a vX.Y.Z -m vX.Y.Z
   git push origin vX.Y.Z
   ```

5. Build packages locally from a clean tree:

   ```sh
   git status --short
   ./scripts/package-local.sh nix deb rpm flatpak
   ```

   Include `archive` too when a tarball release artifact is wanted:

   ```sh
   ./scripts/package-local.sh nix archive deb rpm flatpak
   ```

6. Verify the local artifacts:

   ```sh
   find dist/packages -maxdepth 1 -type f -printf '%f %s bytes\n' | sort
   sha256sum dist/packages/*
   result-lumen/bin/lumen --version
   ```

7. Upload the locally built artifacts to the existing remote tag:

   ```sh
   ./scripts/upload-release-assets.sh vX.Y.Z
   ```

8. Verify the GitHub release:

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
