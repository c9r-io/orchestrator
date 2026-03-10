#!/usr/bin/env bash

set -euo pipefail

TARGET="${1:-i686-unknown-linux-gnu}"
TMPDIR="$(mktemp -d)"

cleanup() {
  rm -rf "${TMPDIR}"
}

trap cleanup EXIT

rustup target add "${TARGET}" >/dev/null

mkdir -p "${TMPDIR}/src"

cat > "${TMPDIR}/Cargo.toml" <<'EOF'
[package]
name = "rlimit-abi-probe"
version = "0.1.0"
edition = "2021"

[dependencies]
libc = "0.2"
EOF

cat > "${TMPDIR}/src/main.rs" <<'EOF'
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimitResource = libc::__rlimit_resource_t;

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimitResource = libc::c_int;

fn rlimit_resource(resource: u64) -> Result<RlimitResource, &'static str> {
    RlimitResource::try_from(resource).map_err(|_| "unsupported rlimit resource selector")
}

fn main() {
    let _setrlimit: unsafe extern "C" fn(RlimitResource, *const libc::rlimit) -> libc::c_int =
        libc::setrlimit;

    let _ = rlimit_resource(libc::RLIMIT_AS as u64).unwrap();
    let _ = rlimit_resource(libc::RLIMIT_CPU as u64).unwrap();
    let _ = rlimit_resource(libc::RLIMIT_NPROC as u64).unwrap();
    let _ = rlimit_resource(libc::RLIMIT_NOFILE as u64).unwrap();
}
EOF

cargo check --quiet --target "${TARGET}" --manifest-path "${TMPDIR}/Cargo.toml"

cat <<EOF
Validated libc setrlimit RLIMIT_* typing for ${TARGET}.

If you also want a full repository cross-target check, install a matching C toolchain first.
Example:
  CC_${TARGET//-/_}=i686-linux-gnu-gcc cargo check -p agent-orchestrator --target ${TARGET}
EOF
