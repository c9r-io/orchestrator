# FR-116-A: Codex Driver 会话续接语义验证（Follow-up）

## 类型: Follow-up Task（FR-116 子项，非独立 FR）

## 优先级: P3

## 状态: Deferred

## 归属

- 父项：FR-116 Agent Driver 抽象（已闭环，设计现由 `docs/design_doc/orchestrator/127-agent-driver-abstraction.md` 承载）
- 相关：`docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`（Option A 供应商协议适配器）

## Background

FR-116 交付的 `codex/cli` driver 在能力描述符中声明 `session_resume: true`（`crates/orchestrator-runner/src/driver/registry.rs`），命令构造 `build_codex_command`（`crates/orchestrator-runner/src/driver/providers.rs`）在存在 `session_ref` 时生成：

```
codex exec resume <token> --json …
```

**该 flag 拼法未经实测。** FR-116 交付时即标注为未验证项：`claude/cli` 的 stream-json 续接经 e2e 验证，而 codex 的 `resume` 子命令语义、token 形态与 `--json` 事件 schema 均只按推断实现，没有 conformance fixture 支撑。

`codex/cli` 是**可运行的**（`create_driver` 对 `(Codex, Cli)` 返回 `CodexCliDriver`，仅 SDK transport 被 build-time 门禁拦截）。因此一旦某个 workflow 选择 `codex/cli` 且声明 `session_resume` 需求，实际续接会在未验证路径上执行，可能静默失败或在错误的 boundary 重启。

## Why P3

- 当前无生产 workflow 使用 codex driver；`claude/cli` 是默认与主路径。
- 不阻塞 FR-116 主线闭环——能力门禁、沙箱不变量、事件投影均已独立验证。
- 影响面局限于「显式选择 codex driver 且启用跨 step 续接」这一尚未启用的组合。

## Scope

- 用真实 `codex` CLI 做一次 spike：确认 `codex exec resume <token>` 的确切子命令形态、token 来源字段、`--json` 事件流是否可映射到 `DriverEvent`。
- 依据结果修正 `build_codex_command` 与 `ProcessSession` 的 codex 事件映射。
- 补一个 recorded-protocol conformance fixture，pin 到验证过的 codex 版本（对齐 claude fixture 的做法）。
- 更新 DD-127 中 codex driver 的能力描述说明。

## Interim Safe Option（治理时二选一）

在 spike 完成前，任选其一以消除潜在静默失败：

1. **保守**：将 `codex/cli` 的 `session_resume` 暂设为 `false`。apply 层会把任何要求续接的 codex step 在启动前拒绝（`driver_session_resume_required`），fail-closed，语义清晰。
2. **维持现状 + 显式警示**：保留 `session_resume: true`，但在 guide 与 DD-127 中标注 codex 续接为 experimental/unverified，并接受首次接通时可能需要修正命令拼法。

推荐 **选项 1**，与项目既有的 fail-closed 惯例（`SideEffectClass` 默认 non-idempotent、歧义匹配 fail closed）一致。

## Acceptance Criteria

- 存在一个针对真实 codex 版本的续接 conformance 测试，且通过。
- `build_codex_command` 生成的 resume 命令与实测语义一致。
- codex `--json` 事件到 `DriverEvent` 的映射有 fixture 覆盖。
- DD-127 的 codex 能力描述与实现一致；若采用 Interim 选项 1，则在验证完成后再将 `session_resume` 翻回 `true` 并同步文档。

## Notes

本文档为 FR-116 的低优先级挂靠任务，不占用新的 FR 编号。完成后可直接删除本文件，并在 DD-127 记录结论——遵循 `已闭环并删除` 惯例。
