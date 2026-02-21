# Orchestrator - 配置创建流程测试

**Module**: orchestrator
**Scope**: 验证通过 apply 命令创建配置资源的流程
**Scenarios**: 4
**Priority**: High

---

## Background

测试使用 `apply` 命令创建 workspace、agent、workflow 配置资源。

Entry point: `./orchestrator/scripts/orchestrator.sh <command>`

---

## Scenario 1: 创建 Workspace (dry-run)

### Preconditions

- 有效的配置文件存在 (包含基本结构)
- 数据库已初始化

### Goal

验证 dry-run 模式不持久化更改。

### Steps

1. 创建 workspace manifest:
   ```bash
   cd orchestrator
   cat > /tmp/test-ws.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Workspace
   metadata:
     name: test-ws-dryrun
   spec:
     root_path: /tmp/test-ws
     qa_targets:
       - docs/qa
     ticket_dir: docs/ticket
   EOF
   ```

2. Apply with dry-run:
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/test-ws.yaml --dry-run
   ```

3. 验证未创建:
   ```bash
   ./scripts/orchestrator.sh workspace list
   ```

### Expected

- Step 2 输出: `workspace/test-ws-dryrun would be created (dry run)`
- Step 3 列表中不包含 `test-ws-dryrun`

---

## Scenario 2: 创建 Workspace (实际)

### Preconditions

- 同 Scenario 1

### Goal

验证实际创建 workspace。

### Steps

1. 创建实际 workspace:
   ```bash
   cd orchestrator
   mkdir -p /tmp/test-ws
   ./scripts/orchestrator.sh apply -f /tmp/test-ws.yaml
   ```

2. 验证已创建:
   ```bash
   ./scripts/orchestrator.sh workspace info --workspace-id test-ws
   ```

### Expected

- Step 1 输出: `workspace/test-ws created`
- Step 2 显示 workspace 信息，包含正确的 root_path

---

## Scenario 3: 创建完整的最小配置

### Preconditions

- 无 workspace/agent/workflow 配置

### Goal

验证创建一个包含最小必需配置的完整流程。

### Steps

1. 创建最小配置 manifest:
   ```bash
   cd orchestrator
   cat > /tmp/minimal-config.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Workspace
   metadata:
     name: minimal-ws
   spec:
     root_path: /tmp/minimal
     qa_targets: []
     ticket_dir: docs/ticket
   EOF
   
   mkdir -p /tmp/minimal
   ./scripts/orchestrator.sh apply -f /tmp/minimal-config.yaml
   ```

2. 创建 agent:
   ```bash
   cd orchestrator
   cat > /tmp/test-agent.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Agent
   metadata:
     name: test-agent
   spec:
     capabilities:
       - qa
     templates:
       qa: "echo 'test-qa'"
   EOF
   
   ./scripts/orchestrator.sh apply -f /tmp/test-agent.yaml
   ```

3. 创建 workflow:
   ```bash
   cd orchestrator
   cat > /tmp/test-workflow.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Workflow
   metadata:
     name: test-workflow
   spec:
     steps:
       - id: run_qa
         required_capability: qa
         enabled: true
     loop:
       mode: once
     finalize:
       rules: []
   EOF
   
   ./scripts/orchestrator.sh apply -f /tmp/test-workflow.yaml
   ```

4. 验证配置:
   ```bash
   ./scripts/orchestrator.sh config list-agents
   ./scripts/orchestrator.sh config list-workflows
   ```

### Expected

- 所有 apply 成功
- Step 4 显示新创建的 agent 和 workflow

---

## Scenario 4: 资源存在时 apply (更新)

### Preconditions

- Workspace 已存在

### Goal

验证更新已存在的资源配置。

### Steps

1. 更新 workspace:
   ```bash
   cd orchestrator
   cat > /tmp/update-ws.yaml << 'EOF'
   apiVersion: orchestrator.dev/v1
   kind: Workspace
   metadata:
     name: minimal-ws
   spec:
     root_path: /tmp/minimal-updated
     qa_targets:
       - docs/qa
       - docs/security
     ticket_dir: docs/ticket
   EOF
   
   ./scripts/orchestrator.sh apply -f /tmp/update-ws.yaml
   ```

2. 验证更新:
   ```bash
   ./scripts/orchestrator.sh workspace info --workspace-id minimal-ws
   ```

### Expected

- Step 1 输出: `workspace/minimal-ws updated`
- Step 2 显示更新后的 qa_targets

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 创建 Workspace (dry-run) | ☐ | | | |
| 2 | 创建 Workspace (实际) | ☐ | | | |
| 3 | 创建完整的最小配置 | ☐ | | | |
| 4 | 资源存在时 apply (更新) | ☐ | | | |
