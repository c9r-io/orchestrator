# FR-112: Process Console Source Automation UI

## 优先级: P1

## 状态: Proposed

## 依赖: FR-109, FR-110, FR-111, Process Console v1 information architecture

## 计划闭环产物

- `docs/design_doc/orchestrator/123-process-console-source-automation-ui.md`
- `docs/qa/orchestrator/160-process-console-source-automation-ui.md`
- `scripts/qa/test-source-automation-ui.sh`

## Background

YAML/CLI 是可复现配置的基础，但用户的真实需求包括高频地管理多个 badge、Skill template 和 Slack installation。若 operator 必须记忆资源字段、手工比对冲突或从 source event 猜测失败原因，这个能力仍然不是可日常使用的产品前端。

本 FR 在 Process Console 的 Sources 信息架构内增加 Automations 工作区，复用 daemon resource、preview、simulation、route query 和 audit APIs。GUI 不实现自己的 matcher 或 renderer。

## Goals

- 在 Sources 下提供 Templates、Badge Bindings 和 Recent Routes 三个关联视图。
- 支持创建/编辑/复制/预览 SourceTaskTemplate。
- 支持创建/编辑/suspend/resume SourceTaskBinding，并即时显示冲突、权限和引用错误。
- 提供 sample message URL preview 和 non-mutating route simulation。
- 让 operator 从失败 route/Attention 深链到 binding、template、source event 和 task。
- 提供 role-aware mutations、optimistic versioning、audited confirmation and recoverable errors。
- 满足键盘、屏幕阅读器、contrast、reduced motion/transparency 和窄窗口要求。

## Non-goals

- 在 GUI 中管理 Slack OAuth app 安装或显示 SecretStore value。
- 显示/缓存 Slack message body、attachments 或 thread transcript。
- 前端本地渲染生产 goal 或自行决定 binding match。
- 新增一个与 Sources 平级的顶层导航。
- 在 Slack 中发送状态回执。

## Information Architecture

```text
Sources
├── Events
├── Bindings (existing process bindings)
└── Automations
    ├── Task Templates
    ├── Badge Bindings
    └── Recent Routes
```

Task Template detail shows Skill, target workflow/workspace, allowed variables, revision/hash, references and a preview panel。Badge Binding detail shows installation, exact emoji, channel/role policies, template reference, status and match simulation。Recent Routes uses state/reason/task filters and links to Attention/Process Workspace。

## Core User Flows

### Create an automation

1. Select Slack installation。
2. Select/create SourceTaskTemplate and preview with a sample permalink。
3. Enter exact badge name, channel policy and actor roles。
4. Run daemon simulation and review selected workflow/workspace/Skill。
5. Apply with consequence summary and audit reason。

### Diagnose a failed route

1. Open route from Recent Routes or Attention。
2. Inspect safe state timeline and stable reason。
3. Navigate to the pinned binding/template revision。
4. Fix current configuration or credential through authorized external flow。
5. Preview and replay with optimistic version; observe linked task or updated failure。

## Interfaces And Data Changes

- Tauri commands and typed frontend client wrappers for template preview, binding simulation, route list/get/watch/replay and suspend/resume。
- Resource mutations continue through canonical daemon APIs and action audit envelope。
- UI state uses stable resource/source/route/task IDs and reloads authoritative state after mutation。
- No new browser-side persistence of tokens, permalinks or rendered goals beyond bounded view lifecycle。

## Visual And Interaction Constraints

- Reuse existing Process Console tokens, status chips, split panes, dialogs, tables and command patterns。
- Dense operational rows prioritize scanability and contrast; glass/blur is optional with solid fallback。
- Template/binding editor must expose field-level validation and a clear read-only rendered preview。
- Destructive delete and force cleanup require dependency summary; suspend is the preferred reversible action。
- Conflict/no-match/unauthorized/retrying/needs-attention states cannot rely on color alone。
- Narrow mode collapses detail panes without hiding save/cancel/error/replay actions。
- Focus is trapped/restored in dialogs; async results use appropriate live regions without notification spam。

## Key Design Constraints

- GUI never receives or edits Slack token/signing secret values。
- Preview/simulation always calls daemon and is visually marked non-mutating。
- Save/replay buttons are disabled for stale revision until authoritative refresh and user review。
- Read-only roles can inspect safe metadata and route state but cannot mutate, replay or reveal protected permalink。
- Template rendered goal is treated as potentially sensitive and follows redaction/copy policy。
- Existing Sources event/binding navigation remains reachable and behavior-compatible。

## Acceptance Criteria

- [ ] Operator can create a template and badge binding, preview exact goal/action, apply, suspend and resume without editing YAML。
- [ ] Two badges can bind to two different Skill/workflow templates within one Slack installation。
- [ ] Apply-time conflicts, missing references, unknown variables, invalid badge/channel and unauthorized role are shown at the responsible field。
- [ ] Simulation result equals daemon live matcher/renderer semantics and is clearly non-mutating。
- [ ] Recent Routes can filter by state/binding/task and deep-link to source event, Attention and Process Workspace provenance。
- [ ] Retry/replay flow shows consequences, requires reason, handles stale version and never duplicates task。
- [ ] Secret values and raw Slack message content never appear in DOM, Tauri payload snapshots or browser storage。
- [ ] Operator/read-only role visibility and mutation denial are covered by automated tests。
- [ ] Keyboard-only, focus order/trap/restore, accessible names, live regions, contrast, reduced motion/transparency and narrow layout pass。
- [ ] Existing Attention/Processes/Sessions/Sources/System navigation and Process Console fast tests remain green。

## QA Plan

- Frontend unit tests for forms, reducers, validation display, stale state and role visibility。
- Playwright tests for create/edit/preview/bind/suspend/simulate/replay and deep links using mocked Tauri for fast coverage。
- Minimal real Tauri/daemon vertical test verifies production command serialization and error mapping。
- Accessibility scan plus keyboard and narrow-window assertions。
- DOM/storage inspection verifies no token/raw message leakage。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| GUI becomes a second configuration/rendering engine | Daemon-authoritative preview/simulation and typed responses |
| Operational screen becomes too dense | Three related views, progressive detail and stable deep links |
| Read-only user sees protected Slack URL | Role-aware daemon projection; frontend does not infer permission |
| Stale editor overwrites active binding | Resource revision/optimistic concurrency and explicit refresh |
