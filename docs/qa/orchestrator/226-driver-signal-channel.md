---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - A Signal-Killed Step Says What Killed It

**Module**: Driver contract / sandbox observability
**Scope**: that a driver-executed step killed by a signal reports that signal instead of
collapsing to `exit_code: 1`; that a sandbox CPU-limit kill is classified as
`cpu_limit_exceeded` end to end; and that an ordinary non-zero exit is **not** mistaken for
a signal kill
**Scenarios**: 4
**Priority**: High

## Background

A sandbox `max_cpu_seconds` limit was enforced and unobservable: no event, no reason code,
`sandbox_denied: false`, `exit_code: 1`. CPU exhaustion is the one limit that kills the
process rather than making a call fail, so the probe's stderr is empty and the signal was
its only channel — and `DriverEvent::Finished` had no field for one.
See [DD-188](../../design_doc/orchestrator/188-driver-signal-channel.md).

## Safety

The unit scenarios spawn short-lived shells under `tempdir()`. Scenario 3 runs
`test-fr001-sandbox-matrix.sh`, which starts its **own** daemon over a temporary data
directory and writes nothing outside it.

---

## Scenario 1: A signal-killed child reports its signal

**Steps**

```bash
cargo test -p orchestrator-runner shell_driver_reports_the_signal_that_killed_the_child
```

The test runs `kill -s XCPU $$` through the real shell driver and reads the terminal
`DriverEvent::Finished`.

**Expected result**

`exit_signal == Some(libc::SIGXCPU)` and `exit_code == -1`.

This is the assertion whose absence let the defect live. The classifier already had a green
unit test — one that hand-built a `WaitResult` carrying a signal **no production path could
produce**. The gap was between the process and that struct, so a test at the classifier layer
could not see it, however green.

## Scenario 2 (negative): An ordinary failure acquires no signal

**Steps**

```bash
cargo test -p orchestrator-runner shell_driver_reports_no_signal_for_an_ordinary_failure
```

**Expected result**

`exit_code == 3`, `exit_signal == None` for a step that ran `exit 3`.

This guards the direction the repair itself creates. Without it, an implementation that
manufactured a signal whenever an exit code was missing would satisfy Scenario 1 while
classifying ordinary failures as resource kills — the over-reach half of §4.4 shape 10.

## Scenario 3: The CPU limit is classified end to end

**Steps**

```bash
cargo build -p orchestratord -p orchestrator-cli
bash scripts/qa/test-fr001-sandbox-matrix.sh
```

**Expected result**

All **six** sub-cases pass, including `[PASS] sandbox-cpu-limit: cpu_limit_exceeded`.

The gate is the regression test and needed no new assertion: its exemption was written to
**fail** the moment the event started arriving, so deleting it was the whole change. The five
stderr-classified sub-cases must still pass — they do not use the new channel and must not
start depending on it.

Measured through a live isolated daemon, the `step_finished` payload now reads:

```json
{
  "exit_code": -1,
  "sandbox_denied": true,
  "sandbox_denial_reason": "cpu_limit_exceeded",
  "sandbox_resource_kind": "cpu",
  "sandbox_violation_kind": "sandbox_resource_exceeded"
}
```

with a `sandbox_resource_exceeded` event carrying `reason_code=cpu_limit_exceeded`,
`resource_kind=cpu`.

## Scenario 4: `sandbox_denied` needed no semantic change

**Steps**

Read `sandbox_denied` on the CPU case above and compare with the other resource sub-cases.

**Expected result**

`true`, from the same `detect_resource_exceeded` branch every resource kill flows through.

The ticket asked whether `sandbox_denied: false` on a sandbox-killed step was defensible.
It was a symptom rather than a design question — the field read `false` only because the
classification never fired. Nothing about the field's meaning changed, and the six sub-cases
are now consistent.

## Checklist

- [ ] Scenario 1: a signal-killed child reports `Some(SIGXCPU)` and `exit_code: -1`
- [ ] Scenario 2: `exit 3` reports `exit_code: 3` and `exit_signal: None`
- [ ] Scenario 3: all six sandbox sub-cases pass, CPU included
- [ ] Scenario 3: the five stderr-classified sub-cases are unchanged
- [ ] Scenario 4: `sandbox_denied` is `true` for the CPU kill, with no change to the field

## Mutation Evidence

| Mutation | Caught by | Diagnostic |
|---|---|---|
| restore `status.code().unwrap_or(1)` and drop `status.signal()` (the pre-fix line) | Scenario 1 only | `a signal-killed child must report the signal that killed it — left: None, right: Some(24)` |
| manufacture a signal whenever the exit code is absent | Scenario 2 | an ordinary `exit 3` would report a signal it never received |

Scenario 2 staying green under the first mutation is the evidence that the two scenarios
guard opposite directions rather than duplicating each other.

## Known limits

- Only SIGXCPU is mapped to a reason code. Other signals are now recorded but not
  interpreted, because no profile field would make the attribution safe.
- `exit_code` for signalled steps changed from `1` to `-1`, matching the non-driver wait
  path. No gate asserted the old value, but it is user-visible in `driver_finished` and
  `step_finished`.
- Measured on macOS seatbelt only; the Linux backend shares the driver path but was not run.
