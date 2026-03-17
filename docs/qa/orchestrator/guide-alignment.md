---
self_referential_safe: false
---

# 用户指南编译验证对齐

**Scope**: 验证 FR-018 guide-alignment skill 的文档对齐能力，确认 `docs/guide/` EN/ZH 文档与 CLI `--help` 实际输出一致。

## Scenarios

1. 验证 agent 命令组已完整文档化（EN + ZH）：

   ```bash
   grep -c "agent list\|agent cordon\|agent uncordon\|agent drain" docs/guide/07-cli-reference.md
   grep -c "agent list\|agent cordon\|agent uncordon\|agent drain" docs/guide/zh/07-cli-reference.md
   ```

   Expected:

   - 两个文件均返回 >= 4（每个子命令至少出现一次）
   - agent 别名 `ag` 和 `agent ls` 出现在别名表中

2. 验证别名表完整性：

   ```bash
   grep -E "^\| \`(agent|secret key list|db migrations list)\`" docs/guide/07-cli-reference.md
   grep -E "^\| \`(agent|secret key list|db migrations list)\`" docs/guide/zh/07-cli-reference.md
   ```

   Expected:

   - `agent` → `ag`、`agent list` → `agent ls`、`secret key list` → `secret key ls`、`db migrations list` → `db migrations ls` 均出现

3. 验证所有 CLI 标志均已文档化：

   对以下命令运行 `--help` 并逐一核对文档中的标志表：
   - `task list`：`--status`、`--project`、`--output`、`--verbose`
   - `task start`：`--latest`
   - `task logs`：`--follow`、`--tail`、`--timestamps`
   - `task watch`：`--interval`
   - `task trace`：`--verbose`、`--json`
   - `delete`：`--force`、`--dry-run`、`--project`
   - `get`：`--output`、`--selector`、`--project`
   - `check`：`--workflow`、`--output`、`--project`
   - `version`：`--json`
   - `store list`：`--limit`、`--offset`、`--output`、`--project`
   - `store put`：`--task-id`、`--project`
   - `manifest validate`：`--file`、`--project`

   Expected:

   - 每个标志在 EN 和 ZH 文档中均有对应条目

4. 验证 EN/ZH 结构一致性：

   ```bash
   grep "^## " docs/guide/07-cli-reference.md | wc -l
   grep "^## " docs/guide/zh/07-cli-reference.md | wc -l
   ```

   Expected:

   - 两个文件的二级标题数量相同
   - 章节顺序一致

5. 验证 C/S 命令列表包含 agent 生命周期命令：

   ```bash
   grep "agent" docs/guide/07-cli-reference.md | grep -c "orchestrator agent"
   grep "agent" docs/guide/zh/07-cli-reference.md | grep -c "orchestrator agent"
   ```

   Expected:

   - 两个文件中 `orchestrator agent` 命令行数量 >= 4（list/cordon/uncordon/drain 各至少一次）

6. 验证 guide-alignment skill 存在且可被发现：

   ```bash
   test -f .claude/skills/guide-alignment/SKILL.md && echo "OK"
   ```

   Expected:

   - 输出 "OK"
   - Skill 包含五阶段流程（Compile、Parse、Compare、Auto-Fix、Report）

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
