# 协调工具

协调工具让结构化 Agent 请求 daemon 执行测试、更新当前 Item、管理 QA ticket 或生成动态 Item。它用有类型、可审计的调用替代 stdout capture、JSONPath、`post_actions` 和大量只为串联状态而存在的 CEL。

当 Agent 需要依据权威运行态做工作流内决策时使用它。Capability、Agent 选择、sandbox、预算、权限和 Trigger 仍应保留在 manifest 中；这些是治理边界，不能交给 Agent 自己决定。

## 配置支持工具的 Agent

当前生产路径使用 Claude CLI driver。只开放工作流确实需要的工具：

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: qa-coordinator
spec:
  capabilities: [qa_coordination]
  driver:
    provider: claude
    transport: cli
    options:
      permissionMode: governed
      maxTurns: 6
      budgetCapUsd: 0.25
      allowedTools:
        - mcp__orch__run_tests
        - mcp__orch__scan_tickets
        - mcp__orch__create_ticket
        - mcp__orch__mark_item
```

在 Workflow step 中声明工具宿主需求：

```yaml
behavior:
  side_effect_class: workspace_only
  driverRequirements:
    multiTurn: true
    toolHosting: stdio
    workspaceAccess: write
```

如果 Agent 无法满足要求，apply 阶段就会拒绝配置。`allowedTools` 是 daemon 强制执行的白名单，不只是 prompt 提示。

## 可用工具

| 工具 | 用途 | 重要约束 |
|---|---|---|
| `run_tests` | 执行测试并取得结构化通过/失败证据 | target 仅允许 `workspace`、`core`、`runner`、`scheduler`；沿用所选 ExecutionProfile。 |
| `mark_item` | 记录当前 Item 的受治理状态 | 校验当前 Item 和目标状态。 |
| `create_ticket` | 为已证明的失败创建 QA ticket | 同一次 run 内最近一次 `run_tests` 必须失败；复用既有去重逻辑。 |
| `scan_tickets` | 读取当前 active ticket 集合 | 使用 Workspace ticket 目录和既有 scanner。 |
| `generate_items` | 加入运行中发现的新工作项 | 接受 1–100 个唯一的 workspace 相对路径；拒绝不安全或重复输入。 |

`mark_done` 只作为旧 streaming demo 的兼容别名保留；新工作流应使用 `mark_item`。

## 一次调用如何执行

每次 driver run 开始时，daemon 会在随机 loopback 端口启动带 token 认证的 callback。Claude 通过私有、权限为 `0600` 的 MCP 配置启动 `orch-mcp-tools` stdio shim。shim 只转发 JSON-RPC；daemon 校验 token 与白名单、执行工具并返回 typed result。callback 和 token 随本次 run 结束而失效。

每次调用产生四类互补事件：

- `driver_tool_use` 与 `driver_tool_result`：记录 provider 请求了什么、收到了什么。
- `coordination_tool_started` 与 `coordination_tool_completed`：daemon 的权威执行回执。

可以通过常规 task events/logs 或 QA 时查询 `events` 表检查它们。token 和 provider session ID 不会持久化。

## 迁移声明式 Step

1. 找出只用于协调的字段：`prehook`、`captures`、`json_path`、`post_actions`，以及仅用于连接它们的 pipeline variable。
2. 映射到最小工具集合。例如，把 exit-code capture 和 ticket post-action 改为 `run_tests`、必要时 `create_ticket`，最后 `mark_item`。
3. 把工具加入 Agent 的 `allowedTools`，并在 step requirements 中加入 `toolHosting: stdio`。
4. 在 StepTemplate 中说明 Agent 必须建立的结果，不要放入凭据或治理策略。
5. 并排运行 legacy 与 tool 版本；比较 task/item 终态和事件证据，再删除旧 wiring。
6. 记录仍然存在的跨 step 变量，不要自动把它们扩张为通用状态仓库。

完整的新旧对照在 `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`。运行：

```bash
./scripts/qa/test-coordination-collapse.sh
```

## 故障排查

- **看不到工具**：确认 Agent 的 `allowedTools` 中存在完整名称 `mcp__orch__<name>`，且 step 要求 `toolHosting: stdio`。
- **tool is not allowed**：manifest 白名单是最终权限；只在 prompt 中增加工具名不会生效。
- **`create_ticket` 被拒绝**：必须在同一次 run 中先调用 `run_tests`，且只在结果失败时创建 ticket。
- **callback 认证失败**：不要复制或复用 MCP 文件。检查本次 run artifact 是否存在且权限为 `0600`，再查看已脱敏的 driver/coordination 事件。
- **结果仍由旧 CEL 控制**：通过 parity 后再移除过渡协调。CEL 仍受支持，也适合确定性的治理 gate。
- **测试需要任意命令**：`run_tests` 故意使用很小的 target 白名单。应新增经过评审的 typed tool，不要把它变成 shell escape hatch。

实现与安全边界见 [DD-130](../../design_doc/orchestrator/130-coordination-collapse-mcp-tools.md)，可复现验收见 [QA-168](../../qa/orchestrator/168-coordination-collapse-mcp-tools.md)。
