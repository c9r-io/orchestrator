---
self_referential_safe: false
---

# Orchestrator - 配置创建流程测试

**Module**: orchestrator
**Scope**: 验证通过 apply 命令创建配置资源的流程
**Scenarios**: 4
**Priority**: High

---

## Background

测试使用 `apply` 命令创建 workspace、agent、workflow 配置资源。

Entry point: `orchestrator <command>`

---

## Scenario 1: 创建 Workspace (dry-run)

### Preconditions

- 有效的配置文件存在 (包含基本结构)
- 数据库已初始化
- Note: `workspace list` requires an applied config in SQLite. After `init` only (without apply), commands that need config will fail with "config is not initialized"
- Dry-run apply (`apply --dry-run`) does NOT persist config

### Goal

验证 dry-run 模式不持久化更改。

### Steps

1. 创建 workspace manifest:
   ```bash
   cat > /tmp/test-ws.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: test-ws-dryrun
   spec:
     root_path: /tmp/test-ws
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   EOF
   ```

2. Apply with dry-run:
   ```bash
   orchestrator apply -f /tmp/test-ws.yaml --dry-run
   ```

3. 验证未创建:
   ```bash
   orchestrator workspace list
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
   mkdir -p /tmp/test-ws
   orchestrator apply -f /tmp/test-ws.yaml
   ```

2. 验证已创建:
   ```bash
   orchestrator workspace info test-ws-dryrun
   ```

### Expected

- Step 1 输出: `workspace/test-ws-dryrun created`
- Step 2 显示 workspace 信息，包含正确的 root_path
- After applying only a workspace (without agents and workflows), read commands like `workspace info` will fail with "active config is not runnable" because agents and workflows are required for a complete config
- Apply workspaces, agents, and workflows before running info commands

---

## Scenario 3: 创建完整的最小配置

### Preconditions

- 无 workspace/agent/workflow 配置
- The workflow manifest must match the expected schema for `apply`. If it uses an incorrect schema, it will fail with "data did not match any variant of untagged enum ResourceSpec". Use the correct Workflow manifest format (apiVersion, kind, metadata, spec with steps, loop, finalize)

### Goal

验证创建一个包含最小必需配置的完整流程。

### Steps

1. 创建最小配置 manifest:
   ```bash
   cat > /tmp/minimal-config.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: minimal-ws
   spec:
     root_path: /tmp/minimal
     qa_targets: [docs/qa]
     ticket_dir: fixtures/ticket
   EOF
   
   mkdir -p /tmp/minimal
   orchestrator apply -f /tmp/minimal-config.yaml
   ```

2. 创建 agent:
   ```bash
   cat > /tmp/test-agent.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: test-agent
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"test-qa\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   EOF
   
   orchestrator apply -f /tmp/test-agent.yaml
   ```

3. 创建 workflow:
   ```bash
   cat > /tmp/test-workflow.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: test-workflow
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
     finalize:
       rules: []
   EOF
   
   orchestrator apply -f /tmp/test-workflow.yaml
   ```

4. 验证配置:
   ```bash
   orchestrator get agents
   orchestrator get workflows
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
   cat > /tmp/update-ws.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: minimal-ws
   spec:
     root_path: /tmp/minimal-updated
     qa_targets:
       - docs/qa
       - docs/security
     ticket_dir: fixtures/ticket
   EOF
   
   orchestrator apply -f /tmp/update-ws.yaml
   ```

2. 验证更新:
   ```bash
   orchestrator workspace info minimal-ws
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
