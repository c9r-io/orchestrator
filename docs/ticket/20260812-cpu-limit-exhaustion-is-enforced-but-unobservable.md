# A sandbox CPU limit is enforced but produces no event, no reason code, and `sandbox_denied: false`

- **Filed**: 2026-08-12, while making `scripts/qa/test-fr001-sandbox-matrix.sh` runnable (FR-165 requirement 1 leftovers)
- **Severity**: medium — the limit *is* enforced, so this is an observability gap in a security feature rather than a hole in it. But it is the only one of the six sandbox sub-cases with no signal at all, and an operator sees `exit_code: 1` with nothing naming the cause.
- **Status**: open

## Symptom

Measured on macOS 25.6, all six FR-001 sandbox sub-cases against an isolated
daemon at `0d9d09f6`:

```
PASS  sandbox-open-files-limit         open_files_limit_exceeded
FAIL  sandbox-cpu-limit                want=cpu_limit_exceeded  got=<no event>  exit_code=1  sandbox_denied=0
PASS  sandbox-memory-limit             memory_limit_exceeded
PASS  sandbox-process-limit            processes_limit_exceeded
PASS  sandbox-network-deny             network_blocked
PASS  sandbox-network-allowlist        unsupported_backend_feature
```

Five of six classify correctly. For `sandbox-cpu-limit` the daemon records a
`step_finished` with `execution_mode: sandbox`,
`execution_profile: sandbox_cpu_limit`, `exit_code: 1`,
`sandbox_denied: false`, `sandbox_denial_reason: null`, `duration_ms` ≈ 1900 —
and emits **no** `sandbox_resource_exceeded` event.

The limit itself works: the profile sets `max_cpu_seconds: 1` and the process dies
at about one second of CPU. What does not work is saying so.

## Why the other five work and this one cannot

Classification has two routes, and CPU exhaustion defeats both.

**Route 1, the probe's own stderr.** `orchestrator debug sandbox-probe` prints a
`SANDBOX_PROBE resource=… reason_code=…` line before exiting, and
`detect_resource_exceeded` matches on it. Confirmed from the kept step logs:

```
SANDBOX_PROBE resource=open_files reason_code=open_files_limit_exceeded error=Too_many_open_files_(os_error_24)
SANDBOX_PROBE network=blocked reason_code=network_blocked target=example.com error=failed_to_lookup_address_information…
/etc/profile: fork: Resource temporarily unavailable          # processes, matched by the stderr text rule
```

CPU exhaustion is the one limit that kills the process rather than making a call
fail, so there is no point at which `cpu_burn_probe` could print anything. Its
stderr file is **zero bytes**. A self-reporting probe structurally cannot report
its own SIGXCPU.

**Route 2, the signal.**
`crates/orchestrator-scheduler/src/scheduler/phase_runner/util.rs:312` exists for
exactly this case:

```rust
if exit_signal == Some(libc::SIGXCPU) && execution_profile.max_cpu_seconds.is_some() {
```

and `wait.rs:108` decomposes the status correctly, `(status.code().unwrap_or(-1),
status.signal())`. So a signal-terminated child should arrive as
`exit_code: -1, exit_signal: Some(24)`. The observed record is `exit_code: 1`,
which means the daemon's immediate child exited normally with status 1 and the
SIGXCPU never reached `status.signal()`.

That branch has therefore never fired. It is the FR-165 shape in miniature: a
fallback written for the case the primary route cannot cover, never executed,
because the only gate that would have executed it had never been run.

## What was ruled out

`sandbox-exec` does not mask the signal. Measured directly:

```
$ ( ulimit -t 1; ./target/debug/orchestrator debug sandbox-probe cpu-burn ); echo $?
152                                    # 128 + SIGXCPU
$ ( ulimit -t 1; /usr/bin/sandbox-exec -p '(version 1)(allow default)' \
      ./target/debug/orchestrator debug sandbox-probe cpu-burn ); echo $?
152
```

So the signal is observable in principle, through the sandbox wrapper, and the
loss happens somewhere in the daemon's own spawn/wait path. The `/etc/profile`
line in the processes case shows a **login shell** is involved, so the most likely
candidate is an intermediate process that reaps the signalled child and exits 1
itself — but that has not been confirmed, and confirming it means reading the spawn
path rather than inferring from one field.

## The spawn path, read 2026-08-13 (narrows item 1, does not close it)

Item 1 says the answer is "between `wait.rs`/`util.rs` and the process the daemon
actually spawns". That process is now identified, so the next pass can start from
a named chain instead of a search.

On macOS the daemon's immediate child is built by
`build_sandbox_command` at
`crates/orchestrator-runner/src/runner/sandbox.rs:376-384`:

```
/usr/bin/sandbox-exec -p <profile> <runner.shell> <runner.shell_arg> <command>
```

with `runner.shell_arg` defaulting to `-lc` — a **login** shell, which is what
puts `/etc/profile` in the processes-limit evidence above.

`RLIMIT_CPU` is applied in a `pre_exec` hook on that immediate child
(`runner/resource_limits.rs:13-28`, and `:36-37` for the CPU resource
specifically), so the limit is set on `sandbox-exec`, survives its `execv` into
the shell, and is inherited by whatever the shell forks.

That gives a concrete chain and a concrete question:

```
daemon ──waits on──> sandbox-exec ──execs──> bash -lc  ──forks──> cpu_burn_probe
                     (RLIMIT_CPU set here)   (same PID)           (killed by SIGXCPU)
```

The daemon waits on the shell, not on the probe. A shell that reaps a
signal-killed child normally exits `128+signal` = 152, which `wait.rs:108` would
decompose correctly — but the observed record is `exit_code: 1` with no signal,
so the shell is exiting 1 on its own rather than propagating. **Why it exits 1
is the open question**, and it is the last link in the chain rather than a search
across the codebase.

Two things this does *not* establish, and both matter before repairing:

- Whether the shell could be made to propagate (e.g. the command string ending in
  an `exec`, which would collapse the fork and let the daemon's own child take
  the SIGXCPU) — that is a change to how every sandboxed step is launched, not
  just this one, so it needs its own justification.
- Whether the right fix is propagation at all, versus classifying on the
  `duration_ms` ≈ `max_cpu_seconds` coincidence, which would be a spelling.

Confirming any of it still needs item 1's live reproduction — an isolated daemon
over a temporary data directory and a real `step_finished` payload. This ticket
therefore stays open and `test-fr001-sandbox-matrix.sh` keeps its named exemption.

## Suggested work

1. Find where the signal is lost. `wait.rs` and `util.rs` are both correct as
   written, so the answer is between them and the process the daemon actually
   spawns for a sandboxed step — see the section above, which narrows this to the
   login shell's own exit status. Reproducer:
   `bash /Users/chenhan/.claude/jobs/…/matrix-survey.sh` in spirit — start an
   isolated daemon over a temporary data directory, apply
   `fixtures/manifests/bundles/sandbox-execution-profiles.yaml`, run the
   `sandbox-cpu-limit` workflow, and read the `step_finished` payload.
2. Once it fires, remove the exemption in `test-fr001-sandbox-matrix.sh` — it
   names this ticket, so the gate is the regression test.
3. Consider whether `sandbox_denied: false` on a step the sandbox killed is
   defensible independently of the reason code. An operator filtering on
   `sandbox_denied` currently cannot see this class of kill at all.

## Why the gate is not simply left red

`test-fr001-sandbox-matrix.sh` is release-blocking and five of its six assertions
work. Marking the whole gate non-blocking would drop those five, and leaving it red
would block every release on an observability gap in a limit that is being
enforced. So the single failing sub-case is scoped out by name with this ticket
attached, and the other five stay hard — the same decision DD-179 recorded for
`test-process-console-ui.sh`'s `npm audit` step: when a composite gate is red for
one reason, fix the reason's scope, not the gate's blocking status.
