#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <release-tag> <target-triple>" >&2
  exit 1
fi

release_tag="$1"
target_triple="$2"
stage_root="dist"
package_root="orchestrator-${release_tag}-${target_triple}"
release_dir="target/${target_triple}/release"
archive_path="${stage_root}/${package_root}.tar.gz"
tmp_root="$(mktemp -d)"
stage_dir="${tmp_root}/${package_root}"

cleanup() {
  rm -rf "${tmp_root}"
}

trap cleanup EXIT

for binary in orchestrator orchestratord; do
  if [[ ! -f "${release_dir}/${binary}" ]]; then
    echo "missing binary: ${release_dir}/${binary}" >&2
    exit 1
  fi
done

mkdir -p "${stage_root}"
mkdir -p "${stage_dir}"

install -m 0755 "${release_dir}/orchestrator" "${stage_dir}/orchestrator"
install -m 0755 "${release_dir}/orchestratord" "${stage_dir}/orchestratord"
install -m 0644 README.md "${stage_dir}/README.md"

tar -czf "${archive_path}" -C "${tmp_root}" "${package_root}"
