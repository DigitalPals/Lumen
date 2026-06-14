# Packaging Lumen

This repository can build local packages without waiting for GitHub Actions.

## Nix

Build the Nix package:

```sh
nix build .#lumen
```

The flake exposes `packages.${system}.lumen`, `packages.${system}.default`,
and `checks.${system}.lumen` for `x86_64-linux` and `aarch64-linux`.

## Local Release Artifacts

Build all local artifacts:

```sh
./scripts/package-local.sh
```

Build selected formats:

```sh
./scripts/package-local.sh nix
./scripts/package-local.sh archive deb rpm
./scripts/package-local.sh flatpak
```

Portable Debian/Ubuntu and Fedora packages are built inside `podman` or
`docker` containers. This avoids shipping binaries linked against Nix store
paths.

Local package builds disable release LTO by default to avoid very slow links:

```sh
LUMEN_RELEASE_LTO=true ./scripts/package-local.sh archive deb rpm
```

Artifacts are written to `dist/packages/`.

## Flatpak

The Flatpak build vendors Cargo dependencies into `dist/flatpak-source` and
then builds with `flatpak-builder`:

```sh
./scripts/package-flatpak.sh
```

The Flatpak package is mainly useful for the settings UI and CLI experiments.
Running a full desktop shell inside a Flatpak sandbox can require compositor
and D-Bus permissions that are less reliable than native packages.

## Uploading

Upload local artifacts to a GitHub release:

```sh
./scripts/upload-release-assets.sh v0.6.0
```

If the release does not exist, the script creates a draft release first.
