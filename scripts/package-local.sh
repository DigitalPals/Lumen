#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

VERSION="$(sed -n '/\[workspace\.package\]/,/\[/{/^version/p}' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(uname -m)"
DIST_DIR="${PROJECT_DIR}/dist/packages"
FORMATS=("$@")

if [[ ${#FORMATS[@]} -eq 0 ]]; then
	FORMATS=(nix archive deb rpm flatpak)
fi

mkdir -p "$DIST_DIR"

export CARGO_PROFILE_RELEASE_LTO="${LUMEN_RELEASE_LTO:-false}"
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${LUMEN_RELEASE_CODEGEN_UNITS:-16}"

has_format() {
	local wanted="$1"
	for format in "${FORMATS[@]}"; do
		[[ "$format" == "$wanted" || "$format" == "all" ]] && return 0
	done
	return 1
}

need_container() {
	if command -v podman >/dev/null 2>&1; then
		echo podman
		return 0
	fi
	if command -v docker >/dev/null 2>&1; then
		echo docker
		return 0
	fi
	echo "podman or docker is required for portable Debian/Fedora packages" >&2
	return 1
}

build_archive_native() {
	cargo build --release --locked --bin lumen --bin lumen-settings
	./scripts/ci/build-archive.sh target/release "$VERSION" "$ARCH"
	mv -f "lumen-${VERSION}-${ARCH}-linux.tar.gz" "$DIST_DIR/"
}

run_container() {
	local image="$1"
	shift
	local engine
	engine="$(need_container)"
	"$engine" run --rm \
		-v "${PROJECT_DIR}:/work" \
		-w /work \
		-e DEBFULLNAME="${DEBFULLNAME:-Jas Singh}" \
		-e DEBEMAIL="${DEBEMAIL:-jaskiratpal.singh@outlook.com}" \
		"$image" \
		bash -lc "$*"
}

build_deb_container() {
	run_container ubuntu:latest '
		set -euo pipefail
		export DEBIAN_FRONTEND=noninteractive
		apt-get update
		apt-get install -y --no-install-recommends \
			build-essential ca-certificates clang cmake curl debhelper \
			desktop-file-utils dpkg-dev git libfftw3-dev libgtk-4-dev \
			libgtk4-layer-shell-dev libgtksourceview-5-dev libpipewire-0.3-dev \
			libpulse-dev libudev-dev libxkbcommon-dev pkg-config
		curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
		. "$HOME/.cargo/env"
		export CARGO_PROFILE_RELEASE_LTO="'"${CARGO_PROFILE_RELEASE_LTO}"'"
		export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="'"${CARGO_PROFILE_RELEASE_CODEGEN_UNITS}"'"
		cargo build --release --locked --bin lumen --bin lumen-settings
		./scripts/ci/build-archive.sh target/release "'"$VERSION"'" "'"$ARCH"'"
		./scripts/ci/build-packages.sh "lumen-'"$VERSION"'-'"$ARCH"'-linux.tar.gz" "'"$VERSION"'" deb
		mkdir -p dist/packages
		cp debbuild/DEBS/*/*.deb dist/packages/
	'
}

build_rpm_container() {
	run_container fedora:latest '
		set -euo pipefail
		dnf -y install \
			clang cmake curl desktop-file-utils fftw-devel gcc git gtk4-devel \
			gtk4-layer-shell-devel gtksourceview5-devel libxkbcommon-devel \
			make pipewire-devel pkgconf-pkg-config pulseaudio-libs-devel \
			rpm-build systemd-devel
		curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
		. "$HOME/.cargo/env"
		export CARGO_PROFILE_RELEASE_LTO="'"${CARGO_PROFILE_RELEASE_LTO}"'"
		export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="'"${CARGO_PROFILE_RELEASE_CODEGEN_UNITS}"'"
		cargo build --release --locked --bin lumen --bin lumen-settings
		./scripts/ci/build-archive.sh target/release "'"$VERSION"'" "'"$ARCH"'"
		./scripts/ci/build-packages.sh "lumen-'"$VERSION"'-'"$ARCH"'-linux.tar.gz" "'"$VERSION"'" rpm
		mkdir -p dist/packages
		cp rpmbuild/RPMS/*/*.rpm dist/packages/
	'
}

if has_format nix; then
	nix build .#lumen -o result-lumen
fi

if has_format archive; then
	build_archive_native
fi

if has_format deb; then
	build_deb_container
fi

if has_format rpm; then
	build_rpm_container
fi

if has_format flatpak; then
	./scripts/package-flatpak.sh "$VERSION"
fi

echo "Artifacts are in $DIST_DIR"
