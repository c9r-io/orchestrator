# Orchestrator - 配置缺失错误处理测试

**Module**: orchestrator
**Scope**: 验证工具在配置缺失时的错误处理行为
**Scenarios**: 4
**Priority**: High

---

## Background

重构后的 orchestrator 不再包含硬编码的默认配置。工具依赖于用户提供的配置文件。本测试验证配置缺失时的各种场景。

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: 无配置文件时启动 CLI

### Preconditions

- 无 `config/default.yaml` 文件
- 无数据库或数据库为空

### Goal

验证工具在没有配置文件时给出清晰的错误提示。

### Steps

1. 确保没有配置文件:
   ```bash
   cd orchestrator
   rm -f config/default.yaml data/agent_orchestrator.db
   ```

2. 尝试运行任何 CLI 命令:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

### Expected

- 错误信息: `failed to initialize orchestrator: config file not found: ...`
- 工具正常退出 (非 panic)

---

## Scenario 2: 使用 init 命令初始化

### Preconditions

- 无配置文件

### Goal

验证 `init` 命令可以创建最小可用配置。

### Steps

1. 初始化:
   ```bash
   cd orchestrator
   rm -f config/default.yaml data/agent_orchestrator.db
   ./scripts/orchestrator.sh init
   ```

2. 验证初始化成功:
   ```bash
   ./scripts/orchestrator.sh task list
   ```

3. 查看生成的配置:
   ```bash
   cat config/default.yaml
   ```

### Expected

- Step 1 输出: `Orchestrator initialized at ... with workspace: ...`
- Step 2 显示空的任务列表
- Step 3 显示包含 `echo` agent 和 `basic` workflow 的配置

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
   cd orchestrator
   cat > config/empty.yaml << 'EOF'
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
   ./scripts/orchestrator.sh -c config/empty.yaml task list
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
   cd orchestrator
   echo "invalid: yaml: content: [" > config/broken.yaml
   ```

2. 尝试运行:
   ```bash
   ./scripts/orchestrator.sh -c config/broken.yaml task list
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
