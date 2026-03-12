# Design Doc #32: Automate protoc Dependency (FR-020)

## Status

Implemented

## Context

The project uses gRPC/protobuf for control-plane communication. Building the `orchestrator-proto` crate requires `protoc` (Protocol Buffers compiler), which developers had to install manually before `cargo build` would succeed. CI workflows also had fragmented protoc installation steps across multiple jobs.

## Decision

Use `protobuf-src` as a build dependency in `crates/proto` to automatically compile `protoc` from source when no system `protoc` is available. Support the `PROTOC` environment variable to allow explicit override for CI and power users.

### Key Design Choices

1. **`protobuf-src` over `protoc-bin-vendored`**: `protobuf-src` compiles from source and supports all Rust-supported platforms without maintaining a pre-compiled binary matrix.

2. **`PROTOC` env var priority**: If `PROTOC` is set and the path exists, the build uses it directly, skipping the `protobuf-src` compilation. This allows CI to use pre-installed protoc for speed.

3. **CI unchanged in structure**: CI jobs still install protoc via system packages (apt/brew) or `arduino/setup-protoc`. The `PROTOC` env var is now explicitly passed to build steps to ensure the pre-installed binary is used and `protobuf-src` compilation is skipped.

## Changes

| File | Change |
|------|--------|
| `crates/proto/Cargo.toml` | Added `protobuf-src = "2"` to `[build-dependencies]` |
| `crates/proto/build.rs` | Added PROTOC env detection with `protobuf-src` fallback |
| `.github/workflows/ci.yml` | Added `PROTOC` env var to clippy, test, and cross-compile steps |
| `README.md` | Added Prerequisites section noting protoc is auto-compiled; manual install is optional |

## Trade-offs

- **First build time**: ~2-3 minutes longer when `protobuf-src` compiles from source. Mitigated by cargo build cache (subsequent builds are unaffected) and CI pre-installing protoc.
- **Build dependency size**: `protobuf-src` adds source code to the dependency tree. This only affects build time, not runtime binary size.
