# Two QA gates leak their TMPDIR fixture roots, one of them 41MB per run

- **Observed during**: 2026-08-11 FR-164 governance certification sweep, then re-checked after the sweep exited
- **Severity**: low-medium (no correctness impact; 41MB of repo copies accumulate per `test-qa-gate-surface.sh` run, and CI runners hide it because the runner is discarded)
- **Status**: open

## Symptom

Four directories under `$TMPDIR` survived the sweep's exit. Three are leaks; one
is deliberate:

| Directory | Size | Owner | Verdict |
|---|---|---|---|
| `tmp.HvMAEg3rRf` (`base/`, `f1/`, `f8/`, `f13b/` — each a repo copy with `.git`) | **41MB** | `scripts/qa/test-qa-gate-surface.sh` (governance #4, 15:29) | leak |
| `tmp.juXXaJgvFa` (`coordination-strangler.log`, `consumer-inventory.json`) | 20K | `scripts/qa/test-legacy-coordination-decommission.sh` (#39, 15:40) | leak |
| `tmp.q3JFdd0aik` (`_github_workflows_ci_yml_async_lock_governance`) | 0B | same window | leak (empty) |
| `tmp.Ddu3tY4LSC` (`agent-driver-isolated.log`, `execution-inventory.json`) | 24K | `scripts/qa/test-agent-driver-execution-migration.sh` (#43, 15:42) | **by design** — `EVIDENCE_DIR="${FR126_EVIDENCE_DIR:-$(mktemp -d)}"` retains evidence |

Attribution is by log-mtime correlation against the sweep's per-gate logs plus
directory contents; `f13b`-style fixture naming appears in
`test-qa-gate-surface.sh`.

`test-qa-gate-surface.sh` does call `rm -rf` on inner scratch paths (`$probe`,
`$provided_cache`) but registers no `EXIT` trap over the fixture root, so any
early exit — including a failing assertion — strands the whole 41MB.

## A false lead, recorded so it is not repeated

A first pass used `grep -L 'trap.*rm -rf'` over the 68 scripts calling
`mktemp -d` and concluded **65 of 68 leak**. That number is wrong. Most scripts
clean up as:

```bash
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
```

which the pattern cannot see. Re-derived by asking which scripts have no `trap`
line at all, the population is **3**:
`test-legacy-coordination-decommission.sh`,
`test-agent-driver-execution-migration.sh` (intentional), and
`fixture-target-drift.rb`. So the problem is narrow, not systemic.

This is §4.4 shape 1 — a text pattern standing in for the behaviour it
approximates — committed by the checker rather than the checked. Any repair here
must not re-introduce it: do not gate a fix on `grep 'trap'`.

## Reproduction

```bash
before=$(ls -d "$TMPDIR"/tmp.* 2>/dev/null | wc -l)
./scripts/qa/test-qa-gate-surface.sh >/dev/null 2>&1; echo "exit=$?"
after=$(ls -d "$TMPDIR"/tmp.* 2>/dev/null | wc -l)
echo "$before -> $after"; du -sh "$TMPDIR"/tmp.* 2>/dev/null | sort -h | tail -3
```

## For ticket-fix

1. Classification: Bug (missing cleanup), low severity. Confirm by the
   before/after count above rather than by reading the script.
2. Add an `EXIT` trap over the fixture root in `test-qa-gate-surface.sh` and
   `test-legacy-coordination-decommission.sh`. Leave
   `test-agent-driver-execution-migration.sh` alone — its retention is the
   point, and `FR126_EVIDENCE_DIR` is the documented override.
3. Prefer `cleanup() { rm -rf "$WORK"; }; trap cleanup EXIT` to match the
   convention the other 65 scripts already use, so a future scan of either shape
   finds them.
4. If a guard against recurrence is wanted, make it **behavioural**: have the
   harness count `$TMPDIR` entries before and after each gate and fail on a net
   increase. FR-160 already established residue-counting as the discipline
   (QA 211 records a 25-row before/after residue census); this extends it from
   processes to directories. A static scan for `trap` would be the same proxy
   error described above.
5. Note that CI cannot observe this class of defect at all — the runner is
   thrown away — so whatever guard is chosen has to be one that a local
   certification run exercises.

## Related

The process half of this question is clean: at the time of writing there were
0 `orchestratord` processes, 0 `ppid=1` orphans, 0 listening 19xxx ports and 0
UDS socket files, after ~6 days of machine uptime and a 53-invocation sweep —
so FR-159/FR-160's process-leak work is holding. This ticket is only about
filesystem residue.
