# FR-117-A: 全局 Skill 目录属主/权限位校验（Follow-up）

## 类型: Follow-up Task（FR-117 子项，非独立 FR）

## 优先级: P2

## 状态: Deferred

## 归属

- 父项：FR-117 非代码 Workspace 与全局文件共享（已闭环，设计现由 `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md` 承载）
- 安全条款：`docs/security/authorization/02-file-sharing-ceiling.md`（Scenario 5 目前为人工验证步骤）

## Background

FR-117 的 `fileSharing.globalSkills` 目录以只读方式挂入**每一个** task 沙箱，因此它是一个共享供应链面：任何能写入该目录的主体，都能向所有 task 注入代码。

当前 `crates/orchestrator-config/src/file_sharing.rs` 的 `resolved_global_skills` 在 load 时校验：路径可规范化、位于 `shareableRoots` 天花板内、且为目录。它**不**校验该目录的属主与写权限位。因此「globalSkills 只能由 daemon 用户可写」目前是**运营要求，而非代码强制**——DD-128 的 Risks 段与 security doc 的 Scenario 5 已如实标注这一点。

## Why P2

- 属于安全底座加固，权重高于普通 follow-up（故为 P2，高于 FR-116-A 的 P3）。
- 但非阻塞：`shareableRoots` 子集约束（SI-1）与只读挂载已到位；本项是纵深防御的补强，堵住"globalSkills 指向 task-writable 或 group/world-writable 目录"这一类错误配置。
- 无 globalSkills 配置时不适用（缺省 deny-all）。

## Scope

- 在 `resolved_global_skills`（load 时）增加属主/权限位校验：
  - 目录属主必须是 daemon 运行用户。
  - 拒绝 group-writable / world-writable 目录。
  - 拒绝任何同时落在 task `work_dir` 或某个 writable profile 路径内的 globalSkills 条目（task-writable 即注入面）。
- 校验失败以稳定错误码拒绝（建议 `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`），并给出可操作的修复提示。
- 将 `docs/security/authorization/02-file-sharing-ceiling.md` 的 Scenario 5 从人工验证升级为可执行断言（`cargo test` 覆盖属主/权限位/task-writable 三种拒绝）。
- 跨平台注意：Unix 用 `MetadataExt` 读 uid/mode；非 Unix 平台的行为需显式定义（建议保守拒绝或明确记录为不支持）。

## Acceptance Criteria

- `resolved_global_skills` 拒绝非 daemon-user 属主、group/world-writable、以及 task-writable 的 globalSkills 目录，返回稳定错误码。
- 至少 3 个单测覆盖上述三类拒绝；合法目录仍通过。
- DD-128 Risks 段中「not yet enforced in code」措辞相应更新为已强制。
- security doc Scenario 5 由人工步骤改为可执行测试引用。

## Notes

本文档为 FR-117 的安全加固挂靠任务，不占用新的 FR 编号。完成后可直接删除本文件，并在 DD-128 记录结论——遵循 `已闭环并删除` 惯例。
