#!/usr/bin/env sh

set -eu

REPO="${INSTALL_ORCHESTRATOR_REPO:-c9r-io/orchestrator}"
BIN_DIR="${INSTALL_ORCHESTRATOR_BIN_DIR:-/usr/local/bin}"
VERSION="${INSTALL_ORCHESTRATOR_VERSION:-latest}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

log() {
  printf '%s\n' "$*"
}

detect_os() {
  os_name="$(uname -s)"
  case "$os_name" in
    Linux)
      printf '%s\n' "unknown-linux-gnu"
      ;;
    Darwin)
      printf '%s\n' "apple-darwin"
      ;;
    *)
      echo "unsupported operating system: $os_name" >&2
      exit 1
      ;;
  esac
}

detect_arch() {
  arch_name="$(uname -m)"
  case "$arch_name" in
    x86_64|amd64)
      printf '%s\n' "x86_64"
      ;;
    aarch64|arm64)
      printf '%s\n' "aarch64"
      ;;
    *)
      echo "unsupported architecture: $arch_name" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [ "$VERSION" != "latest" ]; then
    printf '%s\n' "$VERSION"
    return
  fi

  latest_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")"
  latest_tag="${latest_url##*/}"

  if [ -z "$latest_tag" ] || [ "$latest_tag" = "latest" ]; then
    echo "failed to resolve latest release for ${REPO}" >&2
    exit 1
  fi

  printf '%s\n' "$latest_tag"
}

sha_cmd() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s\n' "sha256sum"
    return
  fi

  if command -v shasum >/dev/null 2>&1; then
    printf '%s\n' "shasum -a 256"
    return
  fi

  echo "missing checksum tool: sha256sum or shasum" >&2
  exit 1
}

verify_checksum() {
  archive_name="$1"
  checksum_file="$2"
  checksum_tool="$(sha_cmd)"
  expected_line="$(awk -v file="$archive_name" '$2 == file { print $0 }' "$checksum_file")"

  if [ -z "$expected_line" ]; then
    echo "checksum entry for ${archive_name} not found" >&2
    exit 1
  fi

  actual_sum="$(sh -c "${checksum_tool} \"${archive_name}\"" | awk '{print $1}')"
  expected_sum="$(printf '%s\n' "$expected_line" | awk '{print $1}')"

  if [ "$actual_sum" != "$expected_sum" ]; then
    echo "checksum verification failed for ${archive_name}" >&2
    exit 1
  fi
}

install_binary() {
  src="$1"
  dest="$2"

  if install -m 0755 "$src" "$dest" 2>/dev/null; then
    return
  fi

  echo "failed to install ${src} to ${dest}" >&2
  echo "set INSTALL_ORCHESTRATOR_BIN_DIR to a writable directory or rerun with elevated privileges" >&2
  exit 1
}

need_cmd curl
need_cmd tar
need_cmd mktemp
need_cmd grep
need_cmd awk
need_cmd install

os_suffix="$(detect_os)"
arch_prefix="$(detect_arch)"
target="${arch_prefix}-${os_suffix}"
release_tag="$(resolve_version)"
archive="orchestrator-${release_tag}-${target}.tar.gz"
checksums="orchestrator-${release_tag}-sha256sums.txt"
base_url="https://github.com/${REPO}/releases/download/${release_tag}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT INT TERM

log "installing ${release_tag} for ${target}"
curl -fsSL "${base_url}/${archive}" -o "${tmp_dir}/${archive}"
curl -fsSL "${base_url}/${checksums}" -o "${tmp_dir}/${checksums}"

(cd "$tmp_dir" && verify_checksum "$archive" "$checksums")
tar -xzf "${tmp_dir}/${archive}" -C "$tmp_dir"

package_dir="${tmp_dir}/orchestrator-${release_tag}-${target}"
if [ ! -d "$package_dir" ]; then
  echo "unexpected archive layout: ${package_dir} not found" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
install_binary "${package_dir}/orchestrator" "${BIN_DIR}/orchestrator"
install_binary "${package_dir}/orchestratord" "${BIN_DIR}/orchestratord"

log "installed orchestrator binaries to ${BIN_DIR}"

# Install Claude Code skills (orchestrator-guide) if available
skills_archive="orchestrator-skills-${release_tag}.tar.gz"
skills_url="${base_url}/${skills_archive}"
if curl -fsSL --head "$skills_url" >/dev/null 2>&1; then
  curl -fsSL "$skills_url" -o "${tmp_dir}/${skills_archive}"
  log "installing orchestrator skills"
  tar -xzf "${tmp_dir}/${skills_archive}" -C "."
  log "installed orchestrator-guide skill to .claude/skills/"
fi

# Install skill templates to ~/.orchestratord/skill-templates/ if available
templates_archive="orchestrator-skill-templates-${release_tag}.tar.gz"
templates_url="${base_url}/${templates_archive}"
data_dir="${ORCHESTRATORD_DATA_DIR:-$HOME/.orchestratord}"
if curl -fsSL --head "$templates_url" >/dev/null 2>&1; then
  curl -fsSL "$templates_url" -o "${tmp_dir}/${templates_archive}"
  mkdir -p "${data_dir}"
  tar -xzf "${tmp_dir}/${templates_archive}" -C "${data_dir}"
  log "installed skill templates to ${data_dir}/skill-templates/"
fi
