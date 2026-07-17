# Orchestrator - Source Task Binding And Badge Matching

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: FR-109 native SourceTaskBinding, deterministic exact matching, conflict rejection, lifecycle mutations, and safe live-route gate
**Related QA**: `docs/qa/orchestrator/157-source-task-binding-badge-matching.md`
**Created**: 2026-07-17
**Last Updated**: 2026-07-17

## Background

FR-107 persists authenticated, provider-neutral Slack reaction events and FR-108 defines trusted SourceTaskTemplate recipes. One Slack installation can receive several badges for code analysis, implementation, or documentation, so a fixed Trigger action cannot safely select the intended template. FR-109 adds the missing governed mapping without contacting Slack or creating a task.

## Goals

- Add a native project-scoped `SourceTaskBinding` with the normal resource lifecycle.
- Select exactly one SourceTaskTemplate from authenticated installation, reaction, target, channel, and actor evidence.
- Reject unreachable or overlapping enabled rules before an atomic config swap.
- Expose the same pure matcher through simulation and live reaction routing.
- Make binding apply/delete/suspend/resume auditable without recording message content or URLs.

## Non-goals

- Resolve Slack permalinks, fetch messages, or manage OAuth installations.
- Render SourceTaskTemplate or create/start a task; FR-110 owns those effects.
- Support wildcard/regex reactions, text-based Skill selection, priority, or implicit specificity.
- Add a binding management UI; FR-112 owns the Process Console surface.

## Scope

- In scope: manifest/config/proto/CRD projection, persistence, cross-resource validation, exact matcher, simulation, live-route decision gate, revision, lifecycle mutations, audit, and reference-safe deletion.
- Out of scope: provider network calls, route retry state, task idempotency, schema migration, and UI interactions.

## Resource Interface

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-code-analysis
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-analyze
    targetKind: message
    channels: [C01234567]
    # allChannels: true # mutually exclusive with channels
  templateRef: analyze-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
```

The binding, Trigger, and SourceTaskTemplate must exist in the same project. `triggerRef` must resolve to a Slack webhook Trigger with an `installationId`. `allowedActorRoles` must contain at least one role reachable from that Trigger's `actorRoles` map.

`channels` is non-empty and bounded to 64 exact identifiers, or `allChannels: true` must be explicitly selected. Omitting both is invalid; setting both is invalid. `allowedActorRoles` is also required and non-empty. These secure defaults prevent an omitted restriction from widening automation authority.

## Trigger Rollout Gate

Slack webhook Trigger configuration adds:

```yaml
spec:
  event:
    source: webhook
    webhook:
      provider: slack
      installationId: T012345
      actorRoles:
        U012345: operator
      reactionRouting: bindings
```

`reactionRouting` accepts `disabled` or `bindings` and defaults to `disabled`. Existing Trigger manifests therefore preserve FR-107 behavior: reactions finish as `ignored/reaction_routing_not_enabled`, even when binding resources exist. The `bindings` mode is Slack-only.

## Matching Contract

The pure matcher consumes trusted normalized evidence:

- project, authenticated provider, and authenticated installation ID;
- normalized `eventKind`, exact reaction name, and target kind;
- normalized channel ID;
- authenticated external actor ID.

It never accepts a caller-supplied role. The role is resolved exclusively from the selected Trigger's `actorRoles` map. Reactions are canonical names without colons and match exactly with case-sensitive semantics. V1 supports only `reaction_added` on `message`.

Candidate evaluation order is suspended state, event kind, reaction, target, channel, actor existence, and allowed role. Results are:

- `matched / binding_matched`: exactly one enabled candidate matches;
- `no_match / <stable reason>`: no candidate matches;
- `ambiguous / binding_ambiguous`: multiple candidates match and execution fails closed.

Safe reasons include `trigger_not_found`, `trigger_ambiguous`, `trigger_suspended`, `reaction_automation_disabled`, `binding_not_found`, `binding_suspended`, `event_kind_mismatch`, `reaction_mismatch`, `target_kind_mismatch`, `channel_not_allowed`, `actor_unknown`, and `actor_role_not_allowed`. Candidate output contains only binding ID, reason, and revision.

## Conflict Detection

Two enabled bindings overlap when they reference the same Trigger, event kind, reaction, and target; their channel sets intersect or either uses `allChannels`; and their allowed role sets intersect. Apply rejects the entire candidate config when an overlap exists. Suspended bindings do not conflict until resume, and resume revalidates the complete project before persistence.

There is no priority or specificity rule. Runtime also retains ambiguity detection in case invalid legacy/raced state reaches the matcher; it never guesses a privileged Skill.

## API And CLI

Typed gRPC methods:

- `SourceTaskBindingSimulate(SourceTaskBindingSimulateRequest)` is read-only.
- `SourceTaskBindingSuspend(SourceTaskBindingMutationRequest)` is an Operator mutation.
- `SourceTaskBindingResume(SourceTaskBindingMutationRequest)` is an Operator mutation.

```bash
orchestrator source binding simulate \
  --project my-project \
  --provider slack \
  --installation T012345 \
  --reaction agent-analyze \
  --channel C01234567 \
  --actor U012345 \
  -o json

orchestrator source binding suspend slack-code-analysis --project my-project
orchestrator source binding resume slack-code-analysis --project my-project
```

Simulation has no provider call, permalink input, template render, task mutation, or route persistence. Live reaction routing builds the identical match input from the authenticated normalized source row and calls the same matcher. In the original FR-109 slice a successful live match stopped before mutation; FR-110 now consumes the selected template and revision through the durable permalink/task route documented in design 121 and QA 158.

## Data And Persistence

No database migration is required. SourceTaskBinding uses the unified `resources` table, project config snapshots, builtin CRD projection, and manifest export. Active config publication remains one `ArcSwap` replacement after complete candidate validation.

The binding revision is a lowercase SHA-256 over normalized resource content. Channel and role lists are sorted before hashing, so semantically equivalent orderings produce the same revision across apply/export and daemon restart. FR-110 can record this revision as provenance.

## Authorization, Audit, And Deletion

- Safe get/describe/list/export/simulation operations require read access.
- Apply, delete, suspend, and resume use canonical action audit envelopes.
- Audit targets contain project, binding/resource ID, action, request hash, status, and revision/result ID. They do not contain reaction message body, permalink, rendered goal, or Slack token.
- Normal deletion of a referenced SourceTaskTemplate or Trigger is blocked with the binding names.
- Admin `--force --force-references` atomically removes the referencing bindings and target.
- Direct binding deletion removes only that binding and is audited.

## Key Design

1. A binding references a Trigger rather than duplicating provider credentials or installation identity; authentication stays at the adapter/Trigger boundary.
2. Actor authorization is derived from Trigger configuration rather than request fields.
3. Exact matching and overlap rejection replace implicit precedence, making configuration reviewable and deterministic.
4. One pure matcher serves simulation and live routing to prevent diagnostic/runtime drift.
5. A content hash, rather than an in-memory generation counter, provides portable restart-stable provenance.

## Alternatives And Tradeoffs

- Extend Trigger with a reaction-to-action map: fewer resources, but mixes authentication, routing policy, and reusable templates and makes independent lifecycle/audit harder.
- Choose the most-specific matching binding: convenient but can silently change privilege when a new broad rule is added.
- Allow empty channels/roles as wildcard: concise but unsafe because omission expands authority.
- Selected approach: explicit native binding, explicit broad-channel opt-in, non-empty roles, and exactly-one-match.

## Risks And Mitigations

- Risk: overlapping badges select an unintended privileged Skill.
  - Mitigation: apply/resume conflict rejection and runtime ambiguity fail-closed.
- Risk: clients forge operator/admin roles.
  - Mitigation: matcher accepts only external actor ID and resolves role from trusted Trigger config.
- Risk: config readers observe mixed generations during mutation.
  - Mitigation: validate a cloned candidate and publish one immutable active snapshot.
- Risk: diagnostics expose Slack content.
  - Mitigation: bounded reason codes and identifiers only; simulation has no message URL/body fields.
- Risk: rollout changes existing reaction behavior.
  - Mitigation: `reactionRouting: disabled` is the additive default and maps to the existing terminal reason.

## Observability

- Source route state: disabled/no-match paths retain stable terminal reasons; an enabled successful match proceeds into the FR-110 durable automation route.
- Audit actions: `source.binding.apply`, `source.binding.delete`, `source.binding.suspend`, and `source.binding.resume`; reference cleanup remains `delete_references`.
- Logs: provider event identifiers and installation values retain the existing hashed logging policy; no message content is added.
- Metrics: no new metric in this slice; existing RPC, routing-state, and action-audit queries provide operational evidence.

## Operations / Release

- Config: no new environment variable, secret, or migration.
- Rollout: deploy daemon and CLI together, apply Trigger with default disabled behavior, apply bindings, validate with simulation, then explicitly set `reactionRouting: bindings`.
- Rollback: set `reactionRouting: disabled`, export/remove binding resources, then roll back the binary. Existing source events and tasks require no migration.
- Compatibility: fixed Trigger routing for non-reaction source events is unchanged; reaction automation remains opt-in through `reactionRouting: bindings`.

## Test Plan

- Unit: exact match/no-match matrix, role derivation, validation, overlap, ambiguity, normalized hash, and secure defaults.
- Integration: native lifecycle, conflict rollback, simulation/live matcher equivalence, suspend/resume hot reload, restart revision, audit privacy, and reference deletion.
- Regression: full workspace tests and clippy, including FR-107 reaction and FR-108 template suites.
- E2E/UI: no UI in FR-109; isolated daemon + CLI is the end-to-end boundary.

## QA Docs

- `docs/qa/orchestrator/157-source-task-binding-badge-matching.md`
- Executable proof: `scripts/qa/test-source-task-binding.sh`

## Acceptance Criteria

- Native binding round-trips through apply/get/describe/export and daemon restart.
- Exact allowed evidence selects one template; wrong evidence returns stable no-match reasons.
- Overlap is rejected atomically and runtime ambiguity fails closed.
- Cross-project, missing, wrong-kind, and unreachable references fail validation.
- Suspend/resume is immediate, restart-safe, and audited.
- Referenced Trigger/template deletion is blocked or explicitly cleaned atomically.
- Simulation and live routing share one matcher.
- Existing reactions remain ignored until explicit rollout; enabled matched reactions are owned by FR-110 design 121 and QA 158.
