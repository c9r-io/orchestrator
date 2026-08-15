---
lifecycle: active
---

# DD-188: The Terminal Driver Event Had No Room For A Signal

**Status**: Released
**QA**: [226](../../qa/orchestrator/226-driver-signal-channel.md)
**Closes**: `docs/ticket/20260812-cpu-limit-exhaustion-is-enforced-but-unobservable.md`

## The problem

A sandbox `max_cpu_seconds` limit was enforced and completely unobservable. The step died,
and the record said `exit_code: 1`, `sandbox_denied: false`, `sandbox_denial_reason: null`,
with no `sandbox_resource_exceeded` event. One of six FR-001 sandbox sub-cases, scoped out
by name in a release-blocking gate. An operator saw a failed step and nothing naming the
cause.

The other five classify on **stderr text** — `too many open files`, `resource temporarily
unavailable`, `cannot allocate memory`. CPU exhaustion is the one limit that *kills the
process* instead of making a call fail, so `cpu_burn_probe`'s stderr is zero bytes and it
structurally cannot self-report. The signal was its only possible channel, and
`phase_runner/util.rs:312` exists for exactly that:

```rust
if exit_signal == Some(libc::SIGXCPU) && execution_profile.max_cpu_seconds.is_some() {
```

## What it was not

The ticket had narrowed it to the login shell — "the daemon waits on `bash -lc`, which exits
1 instead of propagating `128+SIGXCPU`; **why it exits 1 is the open question**". Measured,
that is wrong in every part:

| Measurement | Result |
|---|---|
| bare probe under `ulimit -t 1` | 152 |
| `+ sandbox-exec` | 152 |
| `+ sandbox-exec + bash -lc` (the daemon's actual chain) | 152 |
| `bash -lc` alone / `bash -c` alone | 152 |
| does `sandbox-exec` fork or exec? | **execs** — spawned PID and inner `$$` are identical |
| is `RLIMIT_CPU` applied inside the sandbox? | yes — instrumented through a live daemon, `ulimit -t` = 1, `ulimit -Ht` = 1 |
| is the probe really signal-killed? | yes — `/bin/bash: line 1: 96821 Cputime limit exceeded: 24`, `PROBE-EXIT=152` |

The shell propagates. `sandbox-exec` does not interpose a process. The limit is applied.
The probe dies of SIGXCPU. Every link the ticket suspected was innocent.

## What it was

The signal had nowhere to go after the process died.

- `driver/process.rs` reported `status.code().unwrap_or(1)` for a failed child. A
  signal-killed process has **no** exit code, so `None` became **1**, and `status.signal()`
  was never read.
- `DriverEvent::Finished` carried only `outcome` and `exit_code`. There was no field a
  signal could travel in.
- `phase_runner/wait.rs` therefore hardcoded `exit_signal: None` for the driver path.
- So `util.rs:312` was **unreachable for every driver-executed step** — which is all of them.

The classifier was not broken. It had never once been given an input.

### The unit test that passed the whole time

`detect_sandbox_violation_detects_cpu_signal` existed and was green throughout. It builds a
`WaitResult` by hand:

```rust
detect_sandbox_violation(&profile, &wait_result(1, Some(libc::SIGXCPU)), &path)
```

`Some(libc::SIGXCPU)` is a value no production path could produce. The test proved the
classifier maps a signal to a reason code, which was true and never in question; the gap was
one layer below, between the process and that struct, and nothing looked there. A unit test
that constructs its own input cannot discover that the input never arrives — §4.4's question
asked of a fixture's *inputs* rather than its assertions.

## What it says

`DriverEvent::Finished` gains `exit_signal: Option<i32>`. `driver/process.rs` fills it from
`status.signal()` and reports `exit_code: status.code().unwrap_or(-1)`; `wait.rs` carries it
into `WaitResult.exit_signal`; `record.rs` writes it into the `driver_finished` payload so it
is readable and not merely classifiable. **`util.rs:312` is untouched** — that it now works
unchanged is the evidence the repair is aimed at the right layer.

`-1` rather than `1` matches what the non-driver wait path (`wait.rs:70`) has always reported
for this case. Two wait paths disagreeing about the same event is how the gap stayed
invisible, and a signalled step reporting `exit_code: 1` is indistinguishable from a genuine
exit 1 besides.

Protocol-driven providers pass `None`: they report an outcome, not a process status.

### `sandbox_denied` needed no ruling after all

The ticket's item 3 asked whether `sandbox_denied: false` on a sandbox-killed step is
defensible. It turned out to be a *symptom*, not a design question: `denied: true` is set in
the same `detect_resource_exceeded` branch every resource kill flows through, so the field
read `false` only because the classification never fired. Measured after the fix:
`sandbox_denied: true`, `sandbox_denial_reason: "cpu_limit_exceeded"`,
`sandbox_resource_kind: "cpu"` — consistent with the other four resource cases, with no
semantic change to the field.

### The gate is the regression test

`scripts/qa/test-fr001-sandbox-matrix.sh` no longer scopes the case out. The exemption was
written to fail the moment the event started arriving, so removing it required no new
assertion — all six sub-cases are now hard, and the five that classify on stderr are
unchanged and still pass.

## Known limits

- **Only SIGXCPU is classified.** A step killed by SIGKILL (the OOM killer, an operator, the
  hard limit after SIGXCPU is ignored) now *records* its signal but is not mapped to a reason
  code, because there is no profile field that would make the attribution safe. The signal is
  in the event for a human to read; nothing infers from it.
- **`exit_code` for signalled steps changed from `1` to `-1`.** No gate or QA document
  asserted the old value, and the driver and non-driver paths now agree, but it is a
  user-visible field in `driver_finished` and `step_finished`.
- **Linux is inferred, not measured.** Every measurement here ran against macOS seatbelt.
  `RLIMIT_CPU` and `status.signal()` are POSIX and the Linux sandbox backend goes through the
  same driver path, so the channel should behave identically; it was not run.
- **The stderr-based classifiers keep their proxy shape.** Four of the six sub-cases still
  match on message text, which is a proxy for what happened and would break on a libc
  wording change. Unchanged here, and unguarded.
