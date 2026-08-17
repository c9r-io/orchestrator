# 错误码

orchestrator 以方括号形式打印的机器可读错误码，如
`[driver_config_invalid]`。它们出现在 `apply` 输出、校验诊断与任务日志中，
集中在首次运行路径上。本页是词汇表：每个码的含义、触发条件与处置动作。

以下条目集合在 CI 中与源码派生集合比对
（`scripts/qa/test-error-code-glossary.sh`，经 `qa-doc-lint` 执行）：产品新增
错误码而词汇表缺条目会使构建失败，条目对应的码从源码消失同样失败。CLI 侧入口
为 `orchestrator guide error-codes`。

## `legacy_agent_command_deprecated`

- **含义**：`kind: Agent` manifest 设置了 `spec.command` 但省略
  `spec.driver`——这是 driver 机制之前的已废弃写法。
- **触发**：对此类 manifest 执行 `orchestrator apply`。apply 会成功：警告说明
  该 Agent 在持久化时被提升为显式 `shell/cli` driver。
- **处置**：在 manifest 中补上 typed driver，让提升变成显式声明：

  ```yaml
  spec:
    driver:
      provider: shell
      transport: cli
  ```

  参见 [Agent Driver Model](../agent-driver-model.md)。

## `legacy_agent_execution_removed`

- **含义**：调度器被要求执行一个存储记录中没有 typed driver 的 Agent——该记录
  持久化于 driver 提升机制存在之前。
- **触发**：任务执行选中了这样的 Agent。
- **处置**：重新 apply 该 Agent manifest。apply 会把 command-only 配置提升为
  `shell/cli`，存储记录随之获得 driver。

## `legacy_coordination_removed`

- **含义**：Workflow step 使用了 `behavior.captures`，属于已移除的
  CEL/capture 协调机制（DD-137）。
- **触发**：对携带 `behavior.captures` 的 Workflow 执行 `orchestrator apply`
  或 `manifest validate`。manifest 被拒绝。
- **处置**：删除 `captures` 块，改用 typed driver/tool 结果。参见
  [Coordination Tools](../coordination-tools.md)。

## `legacy_json_path_removed`

- **含义**：Workflow step 使用了 JSONPath 后置动作（`spawn_tasks` /
  `generate_items`），随同一次协调机制坍缩一并移除。
- **触发**：对此类 Workflow 执行 `orchestrator apply` 或 `manifest validate`。
  manifest 被拒绝。
- **处置**：用 typed daemon tools 替换该后置动作。参见
  [Coordination Tools](../coordination-tools.md)。

## `legacy_pipeline_variables_removed`

- **含义**：Workflow step 通过四种已退役的 step 级构件之一书写 pipeline
  变量——`store_inputs`、`store_outputs`、`step_vars`，或 `store_put` 后置
  动作。四者都把作者选定的值送入协调机制坍缩已退役的通用 pipeline 变量表
  （DD-169）。
- **触发**：对此类 Workflow 执行 `orchestrator apply` 或 `manifest validate`，
  包括该构件位于 `chain_steps` 内的情形。manifest 被拒绝，诊断会点名你用的
  是四者中的哪一个。
- **处置**：让 step 自行访问 store，无需任何绑定：

  ```yaml
  command: >-
    LAST_SHA="$(orchestrator store get promotion last_published_sha
    --project {project_id} 2>/dev/null || true)" && ...
  ```

  `{project_id}` 由任务上下文渲染，因此无需向 step 预先注入任何值。对于
  agent step，把同一条命令写进 prompt 让 agent 自己执行。对于 `step_vars`，
  直接把值写进该 step 自己的 command 或 prompt。参见
  [Coordination Tools](../coordination-tools.md)。

## `legacy_runner_executor_removed`

- **含义**：manifest 设置了 `runner.executor: streaming`，一种已移除的执行
  模式。`runner.executor` 仅作为历史 `shell` 值的 parse-only 兼容字段存续。
- **触发**：对携带 `runner.executor: streaming` 的 manifest 执行
  `orchestrator apply`。manifest 被拒绝。
- **处置**：删除 `runner.executor`，改为为每个 Agent 配置 `spec.driver`
  （`shell/cli`、`claude/cli` 或 `codex/cli`）。

## `driver_config_invalid`

- **含义**：Agent 的 `spec.driver` 块自相矛盾或与其 Agent 冲突。消息携带具体
  原因——例如 `driver shell/cli requires agent.spec.command`、给某 provider
  配置了属于其他 provider 的子块、或 `claude driver constructs its command;
  agent.spec.command must be omitted`。
- **触发**：`orchestrator apply` 或涉及该 Agent 的 workflow 校验。
- **处置**：修正消息点名的字段。规则：`shell` driver 保留 `spec.command`；
  `claude`/`codex` driver 不得设置 command；每个 provider 只接受自己的子块
  （`shell:`、`claude:`、`codex:`）。

## `driver_raw_args_unsafe_mode_required`

- **含义**：Agent driver 设置了 `rawArgs`，它绕过 provider 的旗标构造，仅在
  daemon 运行于 unsafe 模式时被接受。
- **触发**：向非 unsafe 模式的 daemon apply 此类 Agent。
- **处置**：删除 `driver.rawArgs`（推荐）；确实需要原始旗标时，以 unsafe 模式
  运行 daemon。

## `driver_multi_turn_required`

- **含义**：Workflow step 声明了多轮 driver 要求，而候选 Agent 的 driver 无法
  维持多轮会话。
- **触发**：apply 时的 workflow 校验；该配对被拒绝。
- **处置**：为该 step 换用具备多轮能力的 driver（Claude CLI），或去掉 step 的
  多轮要求。

## `driver_tool_hosting_required`

- **含义**：step 要求的 hosted tool 传输方式，候选 Agent 的 driver 不提供。
- **触发**：apply 时的 workflow 校验。
- **处置**：选择具备所需 tool 传输的 driver，或把 step 的 `toolHosting` 要求
  改为 driver 支持的取值。

## `driver_session_resume_required`

- **含义**：step 要求会话恢复，候选 driver 无法恢复 provider 会话。
- **触发**：apply 时的 workflow 校验。
- **处置**：换用支持会话恢复的 driver（Claude 或 Codex CLI），或移除
  `sessionResume` 要求。

## `driver_permission_events_required`

- **含义**：step 携带审批门，需要 driver 发出权限请求事件，候选 driver 不具备
  该能力。
- **触发**：apply 时的 workflow 校验。
- **处置**：换用支持权限事件的 driver，或从 step 移除审批门。

## `driver_workspace_sandbox_required`

- **含义**：step 声明了工作区访问，这要求可沙箱化的 driver，而候选 driver 不可
  沙箱化。
- **触发**：apply 时的 workflow 校验。
- **处置**：换用可沙箱化的 CLI driver，或把 step 的 `workspaceAccess` 设为
  `none`。

## `driver_guaranteed_cancel_required`

- **含义**：step 被归类为 `nonIdempotentExternal`，要求具备保证取消语义的
  driver，而候选 driver 只有尽力而为的取消。
- **触发**：apply 时的 workflow 校验。
- **处置**：换用保证取消的 driver，或把外部操作改造为幂等后重新归类该 step。

## `driver_transport_unavailable`

- **含义**：Agent driver 声明了 `transport: sdk`。SDK 传输是保留形态，没有
  实现；`cli` 是唯一可执行的传输。
- **触发**：apply 时的 workflow 校验。
- **处置**：把 driver 的 `transport` 改为 `cli`。

## `empty_change_check`

- **含义**：implement 步骤之后的安全自检发现仓库没有任何变更——
  `git diff --stat HEAD` 为空，继续跑检查套件不会证明任何事情。
- **触发**：任务执行中，implement 类步骤完成后；该 item 以此码失败，见
  `task logs`。
- **处置**：检查 implement agent 的输出。agent 结束时没有产生变更——通常是
  agent 判断目标已完成，或 prompt 没有落到它应当编辑的工作目录。

## `secret_value_placeholder_rejected`

- **含义**：某份 `kind: SecretStore` 清单的值是脱敏占位符 `[ENCRYPTED]` 而不是
  真实密钥。读取路径会对密钥值脱敏，所以由 `get secretstore/<name>` 或
  `describe secretstore/<name>` 得到的清单就是这个样子——它并不携带它看上去
  携带的值。
- **触发**：对这样的清单执行 `orchestrator apply` 或 `manifest validate`。
  清单被拒绝，不写入任何东西。
- **处置**：为每个 key 补上真实值。apply 是整体替换，所以从 `spec.data` 中
  省略某个 key 是**删除**它而不是保持不变——把占位符那几行删掉并不能修复
  一份脱敏清单。读取命令用于查看某个 store 定义了哪些 key，它不是这些值的备份。

## `FILE_SHARING_GLOBAL_SKILL_UNTRUSTED`

- **含义**：某个全局共享 Skill 目录未通过信任检查；daemon 拒绝将其暴露给
  agent。消息中带有目录、原因与建议修复。
- **触发**：加载 file-sharing 配置时，全局 Skill 目录的属主或权限不满足信任
  策略。
- **处置**：按消息中的 `suggested_fix` 处理——通常是修正目录属主或权限，或从
  共享配置中移除不受信任的条目。
