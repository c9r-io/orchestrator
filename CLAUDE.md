# CLAUDE.md

## Forbidden Operations

### Never delete the runtime database

Do NOT run `rm -f ~/.orchestratord/agent_orchestrator.db` or any command that deletes or truncates the runtime database (default location: `~/.orchestratord/agent_orchestrator.db`, overridable via `ORCHESTRATORD_DATA_DIR`).

If you encounter a scenario that seems to require deleting the database, it indicates a bug — the system should provide proper isolation (e.g., project-scoped operations) without destructive resets.

- **During QA testing**: Create a ticket under `docs/ticket/` documenting the scenario and the missing isolation mechanism. Adjust the QA doc to work without database deletion, noting the known issue.
- **During interactive work**: Inform the user that the operation would require deleting the database, explain why this suggests a bug, and let the user decide.

## Starting a daemon

### Never hand-roll the daemon lifecycle, including in throwaway scripts

If a script you write starts `orchestratord`, it must take its start and stop from
`scripts/lib/gate_daemon.sh`. That applies **especially** to one-off probes — a script
under `/tmp`, under `~/.claude/jobs/`, or anywhere else outside this repository. Those
are the scripts that leak, because they are the ones nobody reviews and no gate can see.

```bash
. /Users/…/orchestrator/scripts/lib/gate_daemon.sh   # sourceable by absolute path,
                                                     # installs no traps
gate_daemon_wait_ready "$ORCH"          # ready means "can serve", not "answers"
gate_daemon_stop "$DAEMON_PID"          # SIGTERM → 10s → named SIGKILL → 5s → named failure
```

Do not write `( cd "$X" && "$ORCHD" … & echo $! > pidfile )`. Under bash the `&`
backgrounds the whole `cd && …` list and the final command is not exec'd, so `$!` is the
**wrapper shell**, not the daemon. `pkill -F` then kills the wrapper and **exits 0** —
reporting success — while the daemon is reparented to init and survives; a cleanup's
`rm -rf` proceeds to delete the data directory out from under a live writer. Measured on
2026-08-12: two daemons leaked this way ran for 22 hours holding unlinked inodes, and the
run that started them reported all checks passed. FR-160 removed this shape from 25 gates
and `scripts/qa/test-qa-gate-surface.sh` check 16 keeps it out of `scripts/**` — but that
gate cannot see a file outside the repository, which is why this rule is here and not only
there. See [DD-174](docs/design_doc/orchestrator/174-qa-harness-daemon-teardown.md).

If a probe genuinely cannot use the library, it must still leave nothing behind: record
the daemon's real PID (`pgrep -F` is not a substitute for knowing which process you
started), verify termination rather than assuming it, and never `rm -rf` a data directory
whose writer you have not confirmed dead.
