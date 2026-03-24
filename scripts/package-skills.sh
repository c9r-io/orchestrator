#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <release-tag>" >&2
  exit 1
fi

release_tag="$1"
skill_name="orchestrator-guide"
src_dir=".claude/skills/${skill_name}"
stage_root="dist"
archive_path="${stage_root}/orchestrator-skills-${release_tag}.tar.gz"
tmp_root="$(mktemp -d)"

cleanup() {
  rm -rf "${tmp_root}"
}

trap cleanup EXIT

if [[ ! -d "${src_dir}" ]]; then
  echo "skill directory not found: ${src_dir}" >&2
  exit 1
fi

# Stage skill files preserving directory structure for user installation:
#   .claude/skills/orchestrator-guide/SKILL.md
#   .claude/skills/orchestrator-guide/references/*.md
stage_dir="${tmp_root}/.claude/skills/${skill_name}"
mkdir -p "${stage_dir}/references"

cp "${src_dir}/SKILL.md" "${stage_dir}/SKILL.md"
if [[ -d "${src_dir}/references" ]]; then
  cp "${src_dir}/references/"*.md "${stage_dir}/references/"
fi

mkdir -p "${stage_root}"
tar -czf "${archive_path}" -C "${tmp_root}" ".claude"

echo "packaged skills to ${archive_path}"
