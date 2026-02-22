# Orchestrator - 配置缺失错误处理测试

**Module**: orchestrator
**Scope**: 验证工具在配置缺失时的错误处理行为
**Scenarios**: 4
**Priority**: High

---

## Background

Orchestrator 运行时配置存储于 SQLite。初始化后若 SQLite 中尚无配置，CLI 会要求先执行 `config bootstrap --from <file>`。
本测试验证“未初始化配置”“损坏配置”“空配置”等错误与边界场景。

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: 无配置文件时启动 CLI

### Preconditions

- 删除已有数据库（确保 sqlite 中没有已初始化配置）

### Goal

验证在 sqlite 未初始化配置时，CLI 给出明确引导信息。

### Steps

1. 确保没有配置文件:
   ```bash
   rm -f data/agent_orchestrator.db
   ```

2. 尝试运行任何 CLI 命令:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

### Expected

- 错误信息包含: `orchestrator config is not initialized in sqlite`
- 错误信息包含: `run 'orchestrator config bootstrap --from <file>' first`
- 工具正常退出 (非 panic)

---

## Scenario 2: 使用 init 初始化后仍需 bootstrap

### Preconditions

- sqlite 中无配置

### Goal

验证 `init` 仅初始化目录和数据库，不会自动写入配置。

### Steps

1. 初始化:
   ```bash
   rm -f data/agent_orchestrator.db
   ./scripts/orchestrator.sh init
   ```

2. 验证初始化成功:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

3. 使用最小配置 bootstrap:
   ```bash
   cat > /tmp/bootstrap-minimal.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: basic
   workspaces:
     default:
       root_path: .
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     echo:
       capabilities: [qa]
       templates:
         qa: "echo qa"
   workflows:
     basic:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ./scripts/orchestrator.sh config bootstrap --from /tmp/bootstrap-minimal.yaml
   ./scripts/orchestrator.sh task list
   ```

### Expected

- Step 1 输出: `Orchestrator initialized at ...`
- Step 2 失败并提示需要先 bootstrap
- Step 3 bootstrap 成功后，`task list` 可正常执行

---

## Scenario 3: 配置文件为空时启动 CLI

### Preconditions

- 存在空的 `config/empty.yaml`:
  ```yaml
  runner:
    shell: /bin/bash
    shell_arg: -lc
  resume:
    auto: false
  defaults:
    project: ""
    workspace: ""
    workflow: ""
  workspaces: {}
  agents: {}
  workflows: {}
  ```

### Goal

验证工具可以加载有效但为空的配置文件。

### Steps

1. 创建空配置文件:
   ```bash
   cat > /tmp/empty.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     project: ""
     workspace: ""
     workflow: ""
   workspaces: {}
   agents: {}
   workflows: {}
   EOF
   ```

2. 使用空配置运行:
   ```bash
   ./scripts/orchestrator.sh -c /tmp/empty.yaml task list
   ```

### Expected

- 命令成功执行
- 输出空的任务列表 (无 panic)

---

## Scenario 4: 配置文件损坏时启动 CLI

### Preconditions

- 存在损坏的 YAML 配置文件

### Goal

验证工具在配置文件格式错误时给出清晰的错误提示。

### Steps

1. 创建损坏的配置文件:
   ```bash
   echo "invalid: yaml: content: [" > /tmp/broken.yaml
   ```

2. 尝试运行:
   ```bash
   ./scripts/orchestrator.sh -c /tmp/broken.yaml task list
   ```

### Expected

- 错误信息包含: `failed to parse` 或类似的解析错误
- 工具正常退出

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 无配置文件时启动 CLI | ☐ | | | |
| 2 | 使用 init 命令初始化 | ☐ | | | |
| 3 | 配置文件为空时启动 CLI | ☐ | | | |
| 4 | 配置文件损坏时启动 CLI | ☐ | | | |
