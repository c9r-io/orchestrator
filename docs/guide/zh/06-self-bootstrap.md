# 06 - 自引导

自引导工作流是一种特殊场景：编排器通过 AI 代理修改**自身的源代码**。这需要额外的安全机制来防止系统永久性地损坏自身。

## 2 循环策略

自引导使用 `loop.mode: fixed` 配合 `max_cycles: 2`：

```
Cycle 1 — 生产：    plan → qa_doc_gen → implement → self_test → self_restart
Cycle 2 — 验证：    implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance
```

- **Cycle 1** 聚焦于功能开发。QA 步骤通过预钩子延迟（`when: "is_last_cycle"`）。
- **Cycle 2** 是验证阶段。self_restart 重建二进制后，QA 测试针对新代码运行，工单被修复，文档被审计。

`repeatable: false` 标记在 `plan` 和 `qa_doc_gen` 上确保它们只在 Cycle 1 运行。带有 `repeatable: true` 的步骤（如 `implement`、`self_test`）在两个循环中都运行，允许迭代改进。

## 自引用工作区

自引用工作区声明它指向编排器自身的源码树：

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: self
spec:
  root_path: "."
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
  self_referential: true       # 启用生存机制
```

当 `self_referential: true` 时，引擎强制要求：
- `safety.auto_rollback` 必须为 `true`
- `safety.checkpoint_strategy` 不能为 `none`
- workflow 必须包含启用中的 builtin `self_test` 步骤
- `safety.binary_snapshot` 应为 `true`

如果缺少 required 配置，编排器拒绝启动。缺少 `binary_snapshot` 只会产生 warning，不会阻断启动。

## 4 层生存机制

### 第 1 层：二进制快照

在每个循环开始时，当前发布的二进制被复制到 `.stable` 备份文件。如果后续步骤产生了损坏的二进制，系统可以从此快照恢复。

```yaml
safety:
  binary_snapshot: true
```

### 第 2 层：自测试门控

在 `implement` 修改源代码后，`self_test` 内置步骤运行：

1. `cargo check` —— 编译必须通过
2. `cargo test --lib` —— 单元测试必须通过
3. `manifest validate` —— YAML 清单必须有效

如果任何阶段失败，执行在损坏的二进制被部署前停止。

### 第 3 层：自引用执行保护

QA 文件带有 `self_referential_safe` 元数据标记。预钩子变量 `self_referential_safe` 对于测试编排器自身配置或执行引擎的 QA 文档为 `false` —— 防止编排器在测试时无意间修改自身的安全检查。

```yaml
prehook:
  engine: cel
  when: "is_last_cycle && self_referential_safe"
  reason: "跳过不安全的自引用 QA 文档"
```

### 第 4 层：看门狗脚本

`scripts/watchdog.sh` 脚本充当看门狗。如果编排器进程连续崩溃，看门狗恢复 `.stable` 二进制快照并重启。

自重启机制：`self_restart` 内置步骤重建二进制后，daemon 通过 `exec()` 系统调用原地替换进程（保持 PID 不变）。如果 exec 失败，CLI 前台模式的 exit-code-75 重启循环作为后备。

## 自重启流程

```
Cycle 1：
  implement → 修改源代码
  self_test → cargo check + test
  self_restart → cargo build --release
                → 快照新二进制哈希
                → exit(75)

看门狗检测到退出码 75：
  → 用新二进制重新启动编排器
  → 编排器在 Cycle 2 恢复执行

Cycle 2：
  implement → 审查差异，进行增量改进
  self_test → 再次验证
  qa_testing → 针对新代码运行 QA 场景
  ticket_fix → 修复任何 QA 失败
```

## StepTemplate 配置

自引导使用 StepTemplate 将提示词内容与代理解耦：

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  prompt: >-
    你正在 {source_tree} 项目中工作。
    为以下目标创建计划：{goal}。
    当前差异：{diff}
```

代理的命令使用 `{prompt}` 作为占位符：

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: architect
spec:
  capabilities: [plan, qa_doc_gen]
  command: "claude --print -p '{prompt}'"
```

运行时流程：StepTemplate 解析管道变量 → 结果注入代理命令的 `{prompt}`。

## 代理角色

自引导工作流通常使用专业化的代理：

| 代理 | 能力 | 角色 |
|------|------|------|
| architect | `plan`, `qa_doc_gen` | 规划和 QA 文档设计 |
| coder | `implement`, `ticket_fix`, `align_tests` | 代码生成和修复 |
| tester | `qa_testing` | QA 场景执行 |
| reviewer | `doc_governance`, `review` | 文档审计和代码审查 |

## 完整示例

参见 `fixtures/manifests/bundles/self-bootstrap-mock.yaml` 获取包含所有 StepTemplate、Agent 和 Workflow 定义的完整生产清单。

## 下一步

- [07 - CLI 参考](07-cli-reference.md) —— 命令速查
- [03 - 工作流配置](03-workflow-configuration.md) —— 步骤和循环配置
