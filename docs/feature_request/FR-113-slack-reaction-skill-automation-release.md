# FR-113: Slack Reaction Skill Automation Release Acceptance

## 优先级: P1

## 状态: Proposed

## 依赖: FR-107 through FR-112

## 计划闭环产物

- `docs/design_doc/orchestrator/124-slack-reaction-skill-automation-release.md`
- `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md`
- `docs/guide/slack-reaction-skill-automation.md`
- `scripts/qa/test-slack-skill-automation-release.sh`
- `CHANGELOG.md`（更新）

## Background

FR-107 through FR-112 分别交付 contract、resource、routing、operations 和 UI。最终发布仍需证明这些 slice 在 clean daemon、populated upgrade、real signed webhook、fake Slack API、Tauri bridge 和 GUI 中组成同一个闭环，并提供用户可读的安装、配置、验证、止损和回滚说明。

本 FR 不新增产品语义。它建立 release-complete 边界，防止“每个局部测试都通过，但真实 badge 无法创建正确任务”或“升级后旧 Slack source/process 数据丢失”的组合缺陷。

## Goals

- 建立 clean-tree、fresh-binary aggregate release gate。
- 使用真实 Slack signature bytes 和 fake `chat.getPermalink` API 验证完整垂直链。
- 覆盖两个 badge → 两个 SourceTaskTemplate → 两个 distinct Skill/workflow tasks。
- 证明 retry/concurrency/restart 只产生一个 task，并保留完整 provenance。
- 验证 populated Console/source database 前向升级和 binary rollback data preservation。
- 提供注重可读性的用户指南和 operator runbook。
- 聚合 UI functional、accessibility、RBAC、redaction 和 narrow-layout acceptance。

## Non-goals

- 连接真实生产 Slack workspace 或使用真实 AI provider 产生费用。
- 自动创建 Slack app/OAuth installation。
- 发布 GitHub Release、desktop installer 或 hosted service。
- 实现 outbound Slack progress feedback。
- 引入 destructive down migration。

## Required Release Scenario

The aggregate fixture must prove:

1. Start isolated daemon/data dir and apply SecretStore, Workspace, Agent, Workflows, Slack Trigger, two SourceTaskTemplates and two SourceTaskBindings。
2. Start fake Slack API with deterministic permalink, timeout and rate-limit endpoints。
3. Send signed `reaction_added` for `agent-implement`; verify durable ack before asynchronous routing completes。
4. Observe permalink resolution, exact binding/template selection and one queued/running/completed deterministic task。
5. Verify task goal contains configured Skill invocation + permalink and excludes message body/token/raw payload。
6. Repeat same delivery concurrently and restart daemon at a route checkpoint; verify one task and converged route state。
7. Send `agent-docs` on another message; verify distinct template/Skill/workflow task。
8. Drive one invalid credential or ambiguous binding case into Attention, fix/preview/replay, and verify resolution。
9. Open Sources → Automations and Process Workspace through production Tauri commands; verify provenance/deep links and role behavior。

## Required User Guide

`docs/guide/slack-reaction-skill-automation.md` must explain in Chinese-friendly, task-oriented language:

- mental model: Slack installation, badge, template, binding, route and task;
- Slack app event subscription and required reaction event permission;
- SecretStore setup for signing secret and outbound API token without printing secrets;
- manifest and GUI paths for creating two example automations;
- safe preview/simulation before enablement;
- how deduplication, reaction removal and manual rerun behave;
- where to inspect source event, route, task, timeline and Attention;
- common failures: no match, unauthorized actor/channel, invalid token, rate limit, stale config and duplicate delivery;
- suspend, credential rotation, backup, upgrade, smoke, stop-loss and forward-only rollback;
- privacy boundary: URL only by default, no Slack message body ingestion。

## Interfaces And Data Changes

No new runtime interface is expected. This FR owns:

- aggregate release orchestration script;
- populated migration fixture and compatibility matrix;
- user/operations documentation and changelog;
- cross-slice test wiring and stable artifact/diagnostic output。

If an implementation gap requires new product API/schema, it must be returned to the owning FR rather than silently added here。

## Release And Rollback Rules

- Release gate builds current HEAD binaries before daemon-based QA。
- Each slice script remains independently executable; aggregate gate coordinates and summarizes rather than copying assertions。
- Upgrade backs up SQLite, verifies integrity, drains daemon, applies forward migration and runs smoke checks。
- Normal rollback disables reaction automation writers, deploys prior compatible binary and preserves additive tables/source evidence。
- Database restore is reserved for migration failure/corruption, not ordinary binary rollback。
- Rollback must not delete tasks already created by badge automation。

## Key Design Constraints

- Test fixture uses no paid agent and no real Slack credential/network dependency。
- Fake Slack API behavior matches the official request/response contract required by FR-110。
- All ports, socket/data dirs and processes are isolated with bounded readiness and cleanup traps。
- Aggregate logs exclude signing secret、API token、raw source body、message URL and rendered goal。
- GUI fast mock tests remain, but at least one vertical scenario traverses real Tauri handlers and daemon gRPC。
- Release gate reports per-slice owner, duration, status and retained artifact path on failure。

## Acceptance Criteria

- [ ] Aggregate gate builds fresh daemon/CLI/GUI artifacts and calls FR-107 through FR-112 slice tests。
- [ ] Signed Slack badge event → durable source → permalink → template render → canonical task → Console provenance passes end to end。
- [ ] Two badges select two distinct Skill/template/workflow results within one installation。
- [ ] Duplicate delivery、concurrent route and restart fixture converge to one task per message/badge/binding identity。
- [ ] Rate limit/transient recovery and one actionable Attention replay flow pass。
- [ ] Populated pre-feature database upgrades without losing existing task、session、Attention、handoff、source binding、audit or metrics data。
- [ ] Binary rollback preserves new additive data and stops new automation safely。
- [ ] GUI real-boundary test and fast Playwright accessibility/RBAC/redaction suites pass。
- [ ] User guide supports setup, preview, enable, inspect, diagnose, suspend, upgrade and rollback without consulting design docs。
- [ ] CHANGELOG records capability, migration, secrets/permissions, compatibility, privacy defaults and non-goals。
- [ ] Workspace tests, strict Clippy, GUI unit/build/Playwright, doc lint and aggregate release gate are green from a clean worktree。

## QA Plan

- Coordinate all slice scripts with fresh binaries and isolated resources。
- Add populated migration and previous-binary read/rollback fixtures。
- Run full backend workspace tests and GUI unit/build/Playwright suites。
- Validate guide commands against current `orchestrator guide` output and resource schemas。
- Record deterministic release evidence in QA-161; do not include secrets or private Slack URLs。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Aggregate script becomes an opaque monolith | Call owning slice scripts and preserve their diagnostics |
| Fake Slack behavior diverges from actual contract | Fixture derived from official event/permalink fields and boundary tests |
| Rollback guidance destroys new route/source data | Forward-only default; disable writers; preserve additive tables |
| User guide assumes hidden implementation knowledge | Task-oriented examples, screenshots where stable, troubleshooting by visible state |
