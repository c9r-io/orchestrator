#!/usr/bin/env bash
#
# update-homebrew-formula.sh — generate a Homebrew formula from a GitHub Release
#
# Usage:
#   scripts/update-homebrew-formula.sh <release-tag>        # e.g. v0.1.0
#
# The script:
#   1. Downloads the SHA-256 checksum manifest from the release
#   2. Fills in the formula template (homebrew/orchestrator.rb)
#   3. Writes the result to stdout (pipe to file or tap repo)

set -euo pipefail

RELEASE_TAG="${1:?usage: $0 <release-tag>}"
VERSION="${RELEASE_TAG#v}"  # strip leading 'v'
REPO="${ORCHESTRATOR_REPO:-c9r-io/orchestrator}"

CHECKSUM_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/orchestrator-${RELEASE_TAG}-sha256sums.txt"

TEMPLATE="$(dirname "$0")/../homebrew/orchestrator.rb"
if [[ ! -f "$TEMPLATE" ]]; then
  echo "error: formula template not found at ${TEMPLATE}" >&2
  exit 1
fi

# ── fetch checksum manifest ──────────────────────────────────────────
CHECKSUMS="$(curl -fsSL "$CHECKSUM_URL")" || {
  echo "error: failed to download checksum manifest from ${CHECKSUM_URL}" >&2
  exit 1
}

extract_sha() {
  local target="$1"
  local sha
  sha="$(echo "$CHECKSUMS" | grep "orchestrator-${RELEASE_TAG}-${target}.tar.gz" | awk '{print $1}')"
  if [[ -z "$sha" ]]; then
    echo "error: no checksum found for target ${target}" >&2
    exit 1
  fi
  printf '%s' "$sha"
}

SHA_MACOS_ARM64="$(extract_sha "aarch64-apple-darwin")"
SHA_LINUX_AMD64="$(extract_sha "x86_64-unknown-linux-gnu")"
SHA_LINUX_ARM64="$(extract_sha "aarch64-unknown-linux-gnu")"

# ── render formula ───────────────────────────────────────────────────
sed \
  -e "s/PLACEHOLDER_VERSION/${VERSION}/g" \
  -e "s/PLACEHOLDER_SHA256_MACOS_ARM64/${SHA_MACOS_ARM64}/g" \
  -e "s/PLACEHOLDER_SHA256_LINUX_AMD64/${SHA_LINUX_AMD64}/g" \
  -e "s/PLACEHOLDER_SHA256_LINUX_ARM64/${SHA_LINUX_ARM64}/g" \
  "$TEMPLATE"
