# Self-Bootstrap - Self-Referential Safety Policy Alignment

**Module**: self-bootstrap
**Status**: Approved
**Related Plan**: Align self-referential safety semantics across validation, runtime startup, `orchestrator check`, and `manifest validate`; enforce required protections immediately; keep `binary_snapshot` warning-only.
**Related QA**: `docs/qa/self-bootstrap/10-self-referential-safety-policy-alignment.md`
**Created**: 2026-03-11
**Last Updated**: 2026-03-11

## Background

The repository documentation described self-referential execution as a strict safety mode, but the implementation previously mixed hard failures and warnings inconsistently. In practice, `checkpoint_strategy` was enforced too broadly, while `auto_rollback` and missing `self_test` only produced warnings even for self-referential workspaces. The result was an ambiguous contract at the exact boundary where the orchestrator can modify its own source tree.

## Goals

- Define one authoritative self-referential safety policy
- Apply the same policy to config validation, runtime startup, `orchestrator check`, and `manifest validate`
- Promote required safeguards to hard failures for self-referential workspaces
- Standardize diagnostics so operators see rule ID, actual value, expected value, risk, and suggested fix

## Non-goals

- Introducing a new checkpoint backend
- Changing binary snapshot or rollback implementation mechanics
- Redesigning the broader self-bootstrap workflow

## Scope

- In scope: unified policy evaluation, probe profile alignment, runtime event audit, structured diagnostics, QA/doc synchronization
- Out of scope: self-restart redesign, watchdog changes, schema migrations

## Key Design

1. A dedicated evaluator in `core/src/self_referential_policy.rs` computes policy diagnostics without performing IO or CLI rendering.
2. Base rules apply only when the target workspace is `self_referential: true`.
3. Required rules are:
   - `checkpoint_strategy != none`
   - `auto_rollback == true`
   - at least one enabled builtin `self_test`
4. Recommended-only rule:
   - `binary_snapshot == true`
5. `self_referential_probe` now extends the base rules instead of bypassing them. It still requires `loop.mode=once`, `checkpoint_strategy=git_tag`, task-scoped command steps, and a self-referential workspace, while explicitly allowing builtin `self_test`.

## Alternatives And Tradeoffs

- Keep warnings for `auto_rollback` and missing `self_test`
  - Pro: lower compatibility risk
  - Con: leaves self-bootstrap protection weaker than documented
- Apply probe rules as a separate validator
  - Pro: less refactoring
  - Con: continues policy drift and duplicate error formatting
- Chosen approach: centralize evaluation and enforce required rules immediately
  - Reason: it minimizes future drift and makes every surface render the same contract

## Risks And Mitigations

- Risk: existing self-referential workflows fail immediately after upgrade
  - Mitigation: `orchestrator check` and runtime errors both expose actionable rule IDs and fixes
- Risk: probe workflows become harder to author
  - Mitigation: builtin `self_test` is an explicit allowed exception so probe runs can still include the compile/test gate
- Risk: QA docs drift again
  - Mitigation: add a dedicated regression QA doc and update the older survival enforcement doc in the same change

## Observability

- Events: `self_referential_policy_checked` with `workspace_id`, `workflow_id`, `blocking`, and structured diagnostics
- CLI diagnostics:
  - `orchestrator check` exposes structured diagnostics in JSON/YAML and renders detailed text output
  - `manifest validate` exposes validation diagnostics and returns non-zero on policy failure
- Error contract: startup rejection uses `[SELF_REF_POLICY_VIOLATION]` and embeds per-rule detail lines

## Operations / Release

- Config migration: self-referential workflows must now set `auto_rollback: true` and include builtin `self_test`
- Rollback: revert to the previous binary/config if needed; no DB migration required
- Compatibility: non-self-referential workspaces are intentionally unaffected by the stricter rules

## Test Plan

- Unit tests: base-rule enforcement, non-self-referential bypass, probe profile add-on checks
- Integration tests: `orchestrator check` warning rendering, runtime rejection path, policy audit event creation
- QA docs: `docs/qa/self-bootstrap/10-self-referential-safety-policy-alignment.md`

## QA Docs

- `docs/qa/self-bootstrap/10-self-referential-safety-policy-alignment.md`

## Acceptance Criteria

- Self-referential startup rejects missing checkpoint, disabled auto-rollback, and missing builtin `self_test`
- `binary_snapshot` remains warning-only
- Probe workflows require a self-referential workspace and still permit builtin `self_test`
- QA and guide docs match the implemented contract
