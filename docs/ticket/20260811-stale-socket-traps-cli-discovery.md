# A stale socket inode traps CLI discovery into the wrong diagnosis

- **Observed during**: 2026-08-11 product analysis (UX friction audit)
- **Severity**: medium (after any daemon crash/kill, a same-box CLI misdiagnoses a working TCP control plane as "daemon not running")
- **Symptom**: CLI retries UDS three times and reports `failed to connect to daemon at ... Is the daemon running?` while a TLS control-plane config that would work sits one discovery step later
- **Status**: open

## Mechanism (at 6678144d, re-verify)

`crates/orchestrator-client/src/connect.rs:61-66` — transport step 3 probes
only `socket.exists()`. The daemon removes a stale socket **at bind time**
(`crates/daemon/src/main.rs:990`), so after a crash the inode survives; the
CLI then commits to UDS and never falls through to step 4 (control-plane
config discovery). DD-62 fixed the mirror-image direction; this is the
recorded residual. Bonus defect in the same function: `discover_socket_path`
falls back to `.`/cwd when `dirs::home_dir()` is None (`connect.rs:31`) — a
CWD-dependent path never surfaced to the user.

## For ticket-fix

1. Reproduce: start daemon (UDS), `kill -9` it, confirm inode remains, run
   `orchestrator task list` → observe the misdiagnosis.
2. Likely classification: Bug. Repair direction: replace the existence probe
   with a connect probe, or on UDS connect failure continue to the next
   discovery branch; error text should distinguish "socket exists but nobody
   listening" from "no socket".
3. Note the overlap with FR-163 requirement 2 — if FR-163 is governed first
   this ticket collapses into it; whichever moves first closes both.
