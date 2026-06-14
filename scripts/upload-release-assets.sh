#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
	echo "Usage: $0 <tag>" >&2
	exit 1
fi

TAG="$1"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_DIR"

if ! command -v gh >/dev/null 2>&1; then
	echo "gh is required" >&2
	exit 1
fi

shopt -s nullglob
ASSETS=(dist/packages/*)
if [[ ${#ASSETS[@]} -eq 0 ]]; then
	echo "No assets found in dist/packages" >&2
	exit 1
fi

if [[ -n "$(git status --short)" ]]; then
	echo "Working tree is dirty. Commit release sources before uploading assets." >&2
	exit 1
fi

LOCAL_TAG="$(git rev-parse "$TAG" 2>/dev/null || true)"
REMOTE_TAG="$(git ls-remote --tags origin "refs/tags/$TAG" | awk '{print $1}')"

if [[ -z "$LOCAL_TAG" ]]; then
	echo "Local tag $TAG does not exist" >&2
	exit 1
fi

if [[ -z "$REMOTE_TAG" ]]; then
	echo "Remote tag $TAG does not exist on origin. Push the tag before creating the release." >&2
	exit 1
fi

if [[ "$LOCAL_TAG" != "$REMOTE_TAG" ]]; then
	echo "Local tag $TAG ($LOCAL_TAG) does not match origin/$TAG ($REMOTE_TAG)" >&2
	exit 1
fi

if ! gh release view "$TAG" >/dev/null 2>&1; then
	gh release create "$TAG" --verify-tag --title "$TAG" --notes "Local package build for $TAG"
fi

gh release upload "$TAG" "${ASSETS[@]}" --clobber
