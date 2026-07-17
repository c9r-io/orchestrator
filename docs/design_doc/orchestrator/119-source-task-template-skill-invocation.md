# Orchestrator - Source Task Template And Skill Invocation

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-108 native project-scoped template resource, deterministic renderer, read-only preview, and reference-safe deletion  
**Related QA**: `docs/qa/orchestrator/156-source-task-template-skill-invocation.md`  
**Created**: 2026-07-17  
**Last Updated**: 2026-07-17

## Background

Trigger actions select a workflow and workspace, while StepTemplate controls an individual workflow step's prompt. Neither is a reusable source-to-task recipe that binds a trusted Skill invocation, a task action, and explicitly allowlisted source evidence. FR-108 introduces that missing configuration boundary without enabling badge matching or automatic task creation.

## Goals

- Add a native, project-scoped `SourceTaskTemplate` with complete apply/get/describe/delete/export behavior.
- Keep Skill identity and invocation in trusted administrator configuration.
- Render a bounded task goal from exact allowlisted variables with deterministic revision evidence.
- Expose one daemon-authoritative, read-only preview path shared with future live routing.
- Protect referenced templates with an explicit Admin-only, audited force-cleanup path.

## Non-goals

- Match Slack badges or create tasks from reactions.
- Fetch Slack messages or resolve permalink ownership through Slack APIs.
- Install Skills or execute `skill.invocation` as a shell fragment.
- Add a management UI or replace workflow StepTemplate behavior.

## Scope

- In scope: resource parsing/projection/persistence, project cross-reference validation, pure rendering, public redaction, hot reload, restart persistence, CLI preview, and reference-aware deletion.
- Out of scope for FR-108: native binding matching, live source routing, task idempotency, provider credentials, and GUI flows. FR-109 subsequently adds the native binding matcher without changing template rendering.

## Resource Interface

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: docs
    invocation: "$docs"
    args: ["--concise"]
  action:
    workflow: slack-documentation
    workspace: main
    start: true
    initial_vars:
      origin: slack
  goalTemplate: >-
    {skill_invocation}: use {source_message_url} as the source request
  allowedVariables: [skill_invocation, source_message_url]
```

`goalTemplate` and `allowedVariables` use the resource API's camel-case field names. The nested action retains the existing config field `initial_vars`. The resource is project-scoped and its action references a workflow and workspace in the same project.

## Preview API And CLI

The typed gRPC `SourceTaskTemplatePreview` request accepts template name, project, provider, installation, message URL, and optional event/reaction/target identifiers. Its response contains the rendered Skill descriptor, goal, action, content hash/revision, and warnings.

```bash
orchestrator source template preview docs-from-slack \
  --project my-project \
  --provider slack \
  --installation primary \
  --message-url https://example.slack.com/archives/C123/p1234567890000100 \
  -o json
```

Preview reads exactly one immutable active-config snapshot. It does not persist a source event, source binding, task, or routing attempt. Because preview cannot prove installation ownership without contacting Slack, it returns `sample_url_not_verified_against_installation`.

## Rendering Contract

The supported variables are:

- `skill_name`, `skill_invocation`
- `source_message_url`, `source_provider`, `source_installation_id`
- `source_event_id`, `source_reaction`, `source_target_id`

Variables are exact `{variable}` tokens. `{{` and `}}` emit literal braces. Rendering is single-pass, so source-provided braces or template syntax remain inert text. Unknown, undeclared, duplicate, missing, or malformed tokens fail validation.

Limits are 16 KiB for template/rendered goal, 2 KiB for source URL, 512 bytes for invocation, 16 arguments of 1 KiB each, and 32 initial variables of 2 KiB each. Slack preview URLs require HTTPS, no credentials, a `slack.com` or `*.slack.com` host, and an `/archives/` permalink path.

The revision is the lowercase SHA-256 hash of canonical serialized content. `allowedVariables` is sorted before hashing and `initial_vars` is represented by a sorted map, making the hash stable across YAML key ordering and daemon restart.

## Data And Persistence

No schema migration is required. `SourceTaskTemplate` uses the existing unified `resources` table, config version persistence, project config projection, and manifest export path. Active config reload continues through `ArcSwap`; rendering holds one `Arc` snapshot for the full resolution/render operation, so an update yields either the old or new revision, never mixed fields.

Native `SourceTaskBinding` resources now participate in delete-reference protection. FR-109 owns their schema, exact matching, conflict validation, revision, and lifecycle operations.

## Authorization And Deletion

Normal deletion fails with `FailedPrecondition` when a same-project `SourceTaskBinding.spec.templateRef` or `template_ref` references the template. `--force-references` requires `--force`, Admin authorization, and `ActionAuditContext`. The daemon atomically removes the referring resources and template, then records canonical `delete_references` audit evidence. Public audit payloads contain resource identifiers and hashes, not source URLs or rendered goals.

## Key Design And Tradeoffs

1. A new resource is used instead of extending StepTemplate because the lifecycle is task creation, not workflow-step prompt delivery.
2. Rendering is a pure core function used by preview and reserved for future live routing, avoiding client/runtime drift.
3. Skill invocation remains data in the task goal/context boundary; the source router never interpolates it into a shell command.
4. Full content hashes were chosen over a local counter so revisions survive export/import and restart and can be independently verified.
5. Reference scanning was the compatibility seam later replaced by FR-109's native project map while retaining legacy custom-resource cleanup compatibility.

## Risks And Mitigations

- Risk: untrusted source text triggers nested templating or command execution.
  - Mitigation: exact allowlist, single-pass rendering, bounded inputs, and no shell invocation in this slice.
- Risk: preview leaks source or configured sensitive values.
  - Mitigation: apply effective `RuntimePolicy.runner.redaction_patterns` to every public text field; expose only the content hash in audit.
- Risk: a hot update mixes action and goal from different revisions.
  - Mitigation: resolve and render from one immutable `ArcSwap` snapshot.
- Risk: force deletion orphans future bindings.
  - Mitigation: fail by default; Admin-only atomic cleanup with canonical action audit.

## Observability

- Logs: existing structured RPC/resource logs identify operation and project; rendered goal, source URL, and initial variable values are not logged.
- Audit: forced reference cleanup records request ID, actor/role, project, target, reason, request hash, status, and result identifiers.
- Metrics: no new metric is required for a read-only local renderer; control-plane RPC and error metrics remain authoritative.
- Tracing: the typed RPC boundary is the future span seam; no provider call occurs in FR-108.

## Operations / Release

- Config: no new environment variable or secret is required.
- Forward rollout: deploy the daemon and CLI together before applying `SourceTaskTemplate` manifests.
- Rollback: export and remove SourceTaskTemplate resources (and any future binding references) before rolling back to a binary that does not recognize the kind. There is no database downgrade.
- Compatibility: existing manifests, Trigger routing, StepTemplate rendering, source event ingestion, and ignored Slack reactions are unchanged.

## Test Plan

- Unit tests: spec validation, resource round-trip, escaping/single-pass injection resistance, URL policy, deterministic hashing, redaction, immutable snapshot concurrency, and future binding reference handling.
- Integration QA: isolated daemon and project fixture covering lifecycle, preview, persistence counts, hot update, restart, invalid update rollback, deletion authorization, and audit privacy.
- Regression gates: workspace-wide Rust tests and clippy; existing Trigger/StepTemplate tests remain in the workspace suite.
- E2E/UI: not applicable because FR-108 has no management UI or live routing path.

## QA Docs

- `docs/qa/orchestrator/156-source-task-template-skill-invocation.md`
- Executable proof: `scripts/qa/test-source-task-template.sh`

## Acceptance Criteria

- Native resource lifecycle round-trips without field loss.
- Validation rejects unsafe fields, variables, bounds, URLs, and cross-project or missing references.
- Preview and the shared renderer have one output contract and preview produces no durable mutation.
- Revision is deterministic across ordering, hot reload, and restart.
- Public output is bounded and redacted by RuntimePolicy.
- Referenced delete fails closed; explicit Admin cleanup is atomic and audited.
- Existing Trigger, StepTemplate, source ingestion, and reaction behavior remains compatible.
