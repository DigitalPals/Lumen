#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

if ! command -v flatpak-builder >/dev/null 2>&1; then
	echo "flatpak-builder is required. On NixOS, use: nix develop .#default" >&2
	exit 1
fi

if ! command -v flatpak >/dev/null 2>&1; then
	echo "flatpak is required to install runtimes and export a bundle." >&2
	exit 1
fi

VERSION="${1:-$(sed -n '/\[workspace\.package\]/,/\[/{/^version/p}' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')}"
RUNTIME_VERSION="${FLATPAK_RUNTIME_VERSION:-49}"
RUST_EXTENSION_VERSION="${FLATPAK_RUST_EXTENSION_VERSION:-25.08}"
LLVM_EXTENSION="${FLATPAK_LLVM_EXTENSION:-org.freedesktop.Sdk.Extension.llvm21}"
LLVM_EXTENSION_VERSION="${FLATPAK_LLVM_EXTENSION_VERSION:-25.08}"
APP_ID="com.lumen.Lumen"
DIST_DIR="${PROJECT_DIR}/dist"
SOURCE_DIR="${DIST_DIR}/flatpak-source"
BUILD_DIR="${DIST_DIR}/flatpak-build"
REPO_DIR="${DIST_DIR}/flatpak-repo"
MANIFEST="${DIST_DIR}/${APP_ID}.yml"
BUNDLE="${DIST_DIR}/packages/lumen-${VERSION}-x86_64.flatpak"

rm -rf "$SOURCE_DIR" "$BUILD_DIR" "$REPO_DIR" "$MANIFEST"
mkdir -p "$SOURCE_DIR" "${DIST_DIR}/packages"

tar \
	--exclude=.git \
	--exclude=dist \
	--exclude=target \
	--exclude=result \
	--exclude='result-*' \
	-cf - . | tar -x -C "$SOURCE_DIR"
mkdir -p "$SOURCE_DIR/.cargo" "$SOURCE_DIR/vendor"
(
	cd "$SOURCE_DIR"
	cargo vendor --locked vendor >.cargo/config.toml
)

sed "s|@SOURCE_DIR@|${SOURCE_DIR}|g; s|runtime-version: \"49\"|runtime-version: \"${RUNTIME_VERSION}\"|g" \
	packaging/flatpak/${APP_ID}.yml.in >"$MANIFEST"

flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub \
	"org.gnome.Platform//${RUNTIME_VERSION}" \
	"org.gnome.Sdk//${RUNTIME_VERSION}" \
	"org.freedesktop.Sdk.Extension.rust-stable//${RUST_EXTENSION_VERSION}" \
	"${LLVM_EXTENSION}//${LLVM_EXTENSION_VERSION}"

flatpak-builder --force-clean --user --install-deps-from=flathub --repo="$REPO_DIR" "$BUILD_DIR" "$MANIFEST"
flatpak build-bundle "$REPO_DIR" "$BUNDLE" "$APP_ID"
echo "Built $BUNDLE"
