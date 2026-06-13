#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
	echo "Usage: $0 <target-dir> <version> <arch>" >&2
	exit 1
fi

TARGET_DIR="$1"
VERSION="$2"
ARCH="$3"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_DIR"

"${TARGET_DIR}/lumen" completions bash >completions.bash
"${TARGET_DIR}/lumen" completions zsh >_lumen
"${TARGET_DIR}/lumen" completions fish >lumen.fish

STAGING="lumen-${VERSION}-${ARCH}-linux"
mkdir -p "${STAGING}/icons" "${STAGING}/completions"
cp "${TARGET_DIR}/lumen" "${TARGET_DIR}/lumen-settings" LICENSE "${STAGING}/"
cp -r resources/icons/hicolor "${STAGING}/icons/"
cp completions.bash _lumen lumen.fish "${STAGING}/completions/"
cp resources/lumen.service "${STAGING}/"
cp resources/com.lumen.settings.desktop "${STAGING}/"
cp resources/lumen-settings.svg "${STAGING}/"
tar czf "${STAGING}.tar.gz" "${STAGING}"
