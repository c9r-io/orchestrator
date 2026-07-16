# FR-108: Source Task Template And Skill Invocation Resource

## 优先级: P1

## 状态: Proposed

## 依赖: Existing resource, workflow, workspace, task creation, and hot-reload contracts

## 计划闭环产物

- `docs/design_doc/orchestrator/119-source-task-template-skill-invocation.md`
- `docs/qa/orchestrator/156-source-task-template-skill-invocation.md`
- `scripts/qa/test-source-task-template.sh`

## Background

现有 Trigger action 固定选择一个 workflow/workspace，并以 source summary 作为 goal；现有 StepTemplate 则只负责 workflow 内部某一步的 agent prompt。它们都不能表达“使用某个受治理 Skill，以 Slack message URL 作为输入创建任务”的可复用任务配方。

本需求需要一个 project-scoped、可版本化、可预览的 SourceTaskTemplate。模板位于 source routing 与 canonical task creation 之间：它选择 Skill invocation、workflow/workspace action、goal template 和允许输入，但不替代 workflow 内的 StepTemplate。

## Goals

- 新增 native `SourceTaskTemplate` resource，并支持 apply/get/describe/delete/export round-trip。
- 把 Skill name 与 invocation 作为管理员配置，而不是从外部消息推断。
- 支持 workflow、workspace、start、bounded args/initial vars 和 deterministic goal rendering。
- 只允许显式变量，例如 `{skill_invocation}`、`{source_message_url}`、`{source_provider}`。
- 提供 daemon-authoritative preview/render API，供 CLI、GUI 和 live router 共同使用。
- 为每次渲染生成稳定的 template revision/content hash，供历史 provenance 使用。

## Non-goals

- 定义 Codex、Claude Code 或其他 provider 的全局 Skill 安装机制。
- 读取 Slack event 或解析 permalink。
- 把 Slack message body 自动插入 prompt。
- 替代 Agent capability selection、Workflow 或 StepTemplate。
- 在本 FR 中创建 badge binding 或自动任务。

## Scope

### In scope

- Resource manifest/spec/config/projection/validation and active-config hot reload。
- Project-scoped uniqueness and cross-reference validation for workflow/workspace。
- Skill descriptor (`name`, administrator-controlled `invocation`)。
- Goal template parser、allowlist、size limit、escaping policy 和 pure renderer。
- Template preview response containing rendered goal, selected action, revision and warnings, with no mutation。
- Reference-safe delete semantics and audit events。

### Out of scope

- SourceTaskBinding matcher。
- Slack credentials/network calls。
- Route retries or task idempotency。
- Management UI。

## Proposed Manifest

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: docs
    invocation: "$docs"
  action:
    workflow: slack-documentation
    workspace: main
    start: true
  goalTemplate: |
    {skill_invocation}
    Use this Slack message as the source request: {source_message_url}
  allowedVariables:
    - skill_invocation
    - source_message_url
```

The final field spelling is owned by the design doc. The semantic separation is required: Skill/action are trusted config; source values are bounded render inputs.

## Interfaces And Data Changes

- Add `SourceTaskTemplate` to resource kind dispatch, project config, proto/resource views and CLI kind aliases.
- Add typed render input/output service. Preview must call this same service rather than duplicate rendering in clients.
- Resource revision is derived from canonical serialized content or an explicit monotonic config generation. Task routing stores name plus revision/hash snapshot.
- Deletion must reject an active SourceTaskBinding reference unless an explicit force path is authorized and audited.

## Key Design Constraints

- `skill.invocation` is never concatenated into a shell command by source routing; it becomes bounded task goal/context consumed by the governed workflow/agent.
- Template variables are exact tokens, not arbitrary CEL, shell expansion or nested template evaluation.
- Unknown/missing variables, duplicate allowlist entries, empty Skill/action, oversized output and invalid references fail validation.
- `source_message_url` must pass an HTTPS URL policy and provider/installation consistency checks at live render time.
- Preview may use a clearly marked sample URL; it cannot fetch Slack content.
- Updating a template affects future routes only. Historical tasks retain the recorded hash/revision and rendered goal.
- Existing resource manifests and StepTemplate behavior remain unchanged.

## Acceptance Criteria

- [ ] Valid SourceTaskTemplate YAML applies and round-trips without field loss through spec ↔ config ↔ resource projection。
- [ ] Invalid Skill, workflow, workspace, goal template, unknown variable or oversized output fails with actionable field-level errors。
- [ ] Preview and live renderer produce byte-identical output for the same template revision and input set。
- [ ] Preview is read-only and emits no task/source mutation。
- [ ] Template updates hot-reload atomically; one render observes either old or new revision, never a mixed snapshot。
- [ ] Content hash/revision is deterministic across daemon restart and YAML key ordering。
- [ ] Delete is blocked while referenced by an active binding, with an audited explicit policy for force cleanup。
- [ ] Skill invocation and rendered goal are redacted/size-bounded according to RuntimePolicy before public presentation。
- [ ] Existing StepTemplate and Trigger manifests remain backward compatible。

## QA Plan

- Resource unit tests for parsing, validation, apply/update/delete and round-trip。
- Renderer property tests for missing/unknown variables, escaping, length and deterministic hash。
- Isolated daemon CLI tests for apply/get/describe/export/preview and hot reload。
- Populated config restart fixture proving revision stability and reference enforcement。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| SourceTaskTemplate duplicates StepTemplate semantics | Keep task-creation action/goal separate from workflow-step prompt |
| Skill invocation becomes shell injection | Treat as task data only; no shell evaluation; administrator-owned config |
| GUI preview drifts from runtime | One daemon renderer and typed preview response |
| Template edits rewrite historical meaning | Persist revision/hash and rendered snapshot on route/task provenance |
