# Execution freshness for the gates a human runs.
#
# 56 of this repository's 94 declared gates run on every push; the other 38 are
# `manual-runbook`, executed by a person following an owner QA document. Nothing
# recorded when that last happened. (These three numbers move with the manifest
# and have gone stale once already — they read 52/87/35 until FR-165. Derive
# them with `jq '[.scripts[]|select(.enforcement=="ci-required")]|length'` over
# config/governance/qa-gate-surface.json rather than trusting this sentence.) `ci-job-liveness.json` tracks workflow jobs
# and cannot see these, so a manual gate that stopped being run — or stopped
# working — was indistinguishable from one someone ran this morning. FR-148
# found `test-coordination-collapse.sh` broken since 07-25 and FR-149 found
# `test-wp05-integration.sh` broken since 2026-03-26, four months, discovered by
# reading rather than by any signal.
#
# What this records is *freshness*, not correctness: when a gate last ran, at
# which revision, and what it exited with. The report is advisory in ci.yml and
# never blocks a push — the point of FR-158 is to stop the governance surface
# growing, and a gate that fails CI when a human has not run a runbook lately
# would be one more thing to feed.
#
# FR-165 made the recorded status matter at one point: release.yml runs
# manual-gate-freshness.rb --strict, and there a record counts only if the run
# it describes exited 0 on a clean worktree. `exitStatus` and `worktreeDirty`
# were written here from the start and read by nothing for as long as the
# ledger existed, so a gate recorded as having failed still reported `ok`.
#
# Sourced, not executed, and armed *after* the gate installs its own trap:
#   . "$REPO_ROOT/scripts/lib/gate_runlog.sh"
#   trap cleanup EXIT
#   gate_runlog_arm scripts/qa/test-foo.sh

GATE_RUNLOG_LEDGER_REL="config/governance/manual-gate-freshness.json"

# Composes with the EXIT trap the caller already installed rather than replacing
# it. Most of these gates — 33 of 38 when this line was last derived, and the
# derivation is one `rg -c '^\s*trap .*EXIT'` over the manifest's manual-runbook
# paths — run `trap cleanup EXIT`, and a second bare `trap ... EXIT` silently
# discards the first, which in these scripts means a leaked daemon on a bound
# port, a leaked temp directory, or a leaked ORCHESTRATORD_DATA_DIR. The
# recording is the least important thing either handler does, so it is the one
# that adapts.
#
# Order is record-then-cleanup, because `$?` at the top of the handler is the
# gate's real status and a cleanup routine overwrites it with its own.
gate_runlog_arm() {
    GATE_RUNLOG_TARGET="$1"
    GATE_RUNLOG_ROOT="${2:-${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null)}}"

    local spec previous
    # `trap -p EXIT` prints `trap -- '<handler>' EXIT`, with embedded single
    # quotes already escaped as '\''. Stripping the fixed ends leaves a
    # single-quoted shell word, so `eval` reconstitutes the handler exactly —
    # including handlers that contain quotes, which a naive unquote would break.
    spec="$(trap -p EXIT)"
    if [[ -n "$spec" ]]; then
        previous="${spec#trap -- }"
        previous="${previous% EXIT}"
        eval "gate_runlog_chain() { eval ${previous}; }"
    else
        gate_runlog_chain() { :; }
    fi

    trap 'gate_runlog_on_exit "$?"' EXIT
}

gate_runlog_on_exit() {
    local status="$1"
    gate_runlog_write "$status"
    gate_runlog_chain
    # No `exit` here. An EXIT trap that exits replaces the script's status with
    # its own, and this handler must be invisible to the caller's contract.
    return "$status"
}

# Fails open on a missing recorder and closed on the claim.
#
# If ruby is absent the ledger is left untouched and a line goes to stderr. That
# is deliberate in both directions: refusing to let a QA gate fail because a
# bookkeeping file could not be written, while leaving the ledger saying the gate
# has not run — which is true, as far as anything here can prove. An unrecorded
# run must never read as a recorded one.
gate_runlog_write() {
    local status="$1"
    local ledger="$GATE_RUNLOG_ROOT/$GATE_RUNLOG_LEDGER_REL"

    # Nothing is recorded when no human is present, and that is the ledger's
    # subject rather than an optimisation: `manual-runbook` means "executed by a
    # person following the owner QA document", so a run inside CI is not the
    # thing this file measures. It fails closed — an unrecorded run leaves null,
    # which reads as not-run.
    #
    # It is also a correctness fix rather than a nicety. Two ci-required gates
    # invoke a manual-runbook gate, so without this a CI run writes a tracked
    # file mid-job, and the six ci-required gates that require a clean worktree
    # then fail for the recorder's reason instead of their own. Measured during
    # FR-158's own certification sweep: test-agent-driver-execution-migration.sh
    # recorded a run for test-agent-driver-abstraction.sh and
    # test-agent-driver-production-parity.sh went red on the resulting diff.
    #
    # CiEnv is the repository's one answer to "am I unattended". `CI` alone is a
    # GitHub Actions and Travis convention that a self-hosted runner or a locally
    # driven agent sails straight past, and this file is not the place for a
    # second, narrower copy of that predicate.
    if command -v ruby >/dev/null 2>&1 &&
        ruby -r"$GATE_RUNLOG_ROOT/scripts/lib/ci_env" \
            -e 'exit(CiEnv.unattended? ? 0 : 1)' 2>/dev/null; then
        return 0
    fi

    if ! command -v ruby >/dev/null 2>&1; then
        echo "[gate-runlog] ruby not found; $GATE_RUNLOG_TARGET ran but was not recorded" >&2
        return 0
    fi
    if [[ ! -f "$ledger" ]]; then
        echo "[gate-runlog] $GATE_RUNLOG_LEDGER_REL not found; $GATE_RUNLOG_TARGET ran but was not recorded" >&2
        return 0
    fi

    local revision dirty
    revision="$(git -C "$GATE_RUNLOG_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
    # The ledger excludes itself: a run records into it, so its own presence in
    # the status is not evidence that anything else moved.
    if [[ -n "$(git -C "$GATE_RUNLOG_ROOT" status --porcelain \
        -- ":(exclude)$GATE_RUNLOG_LEDGER_REL" 2>/dev/null)" ]]; then
        dirty=true
    else
        dirty=false
    fi

    GATE_RUNLOG_STATUS="$status" \
    GATE_RUNLOG_REVISION="$revision" \
    GATE_RUNLOG_DIRTY="$dirty" \
    GATE_RUNLOG_PATH="$GATE_RUNLOG_TARGET" \
    ruby -rjson -e '
      ledger_path = ARGV[0]
      data = JSON.parse(File.read(ledger_path))
      gate = ENV["GATE_RUNLOG_PATH"]
      entry = (data["gates"] ||= {})[gate]
      # A gate absent from the ledger is not invented here. The ledger set is the
      # manifest set, reconciled by the gate that reads it; writing a new key
      # would let a run silently create the record proving it ran.
      unless entry
        warn "[gate-runlog] #{gate} is not listed in #{ledger_path}; not recorded"
        exit 0
      end
      entry["lastRun"] = {
        "date" => Time.now.utc.strftime("%Y-%m-%d"),
        "revision" => ENV["GATE_RUNLOG_REVISION"],
        "exitStatus" => Integer(ENV["GATE_RUNLOG_STATUS"]),
        "worktreeDirty" => ENV["GATE_RUNLOG_DIRTY"] == "true"
      }
      File.write(ledger_path, JSON.pretty_generate(data) + "\n")
    ' "$ledger" || echo "[gate-runlog] could not write $GATE_RUNLOG_LEDGER_REL for $GATE_RUNLOG_TARGET" >&2

    return 0
}

# The worktree-cleanliness predicate six gates use, with the freshness ledger
# excluded.
#
# Those gates refuse to run on a dirty tree because their subject is the built
# artifact or the committed manifest set. Recording a run writes to the ledger,
# so without this every one of them would pass on its first run and fail on its
# second — for the recorder's reason, not its own, which is the "fails for the
# fixture's own reason" pathology aimed at a gate. Shared rather than copied six
# times, for the reason scripts/lib/rust_source.rb exists.
gate_runlog_worktree_status() {
    git -C "${1:-${REPO_ROOT:-.}}" status --porcelain \
        -- ":(exclude)$GATE_RUNLOG_LEDGER_REL"
}
