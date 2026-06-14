#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
	echo "Usage: $0 <target-dir> <version> <arch>" >&2
	exit 1
fi

TARGET_DIR="$(readlink -f "$1")"
VERSION="$2"
ARCH="$3"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

STAGING="lumen-${VERSION}-${ARCH}-linux"
STAGING_DIR="${TMPDIR}/${STAGING}"
COMPLETIONS_DIR="${TMPDIR}/completions"
mkdir -p "${STAGING_DIR}/icons" "${STAGING_DIR}/share/lumen" "${STAGING_DIR}/completions" "${COMPLETIONS_DIR}"

"${TARGET_DIR}/lumen" completions bash >"${COMPLETIONS_DIR}/completions.bash"
"${TARGET_DIR}/lumen" completions zsh >"${COMPLETIONS_DIR}/_lumen"
"${TARGET_DIR}/lumen" completions fish >"${COMPLETIONS_DIR}/lumen.fish"

cp "${TARGET_DIR}/lumen" "${TARGET_DIR}/lumen-settings" LICENSE "${STAGING_DIR}/"
cp -r resources/icons/hicolor "${STAGING_DIR}/icons/"
cp -r resources/icons "${STAGING_DIR}/share/lumen/icons"
cp "${COMPLETIONS_DIR}/completions.bash" "${COMPLETIONS_DIR}/_lumen" "${COMPLETIONS_DIR}/lumen.fish" "${STAGING_DIR}/completions/"
cp resources/lumen.service "${STAGING_DIR}/"
cp resources/com.lumen.settings.desktop "${STAGING_DIR}/"
cp resources/lumen-settings.svg "${STAGING_DIR}/"
tar -C "${TMPDIR}" -czf "${PROJECT_DIR}/${STAGING}.tar.gz" "${STAGING}"
