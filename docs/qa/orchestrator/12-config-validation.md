# Orchestrator - 配置验证测试

**Module**: orchestrator
**Scope**: 验证配置验证功能和错误检测
**Scenarios**: 4
**Priority**: High

---

## Background

测试 `manifest validate` 命令和配置错误检测。

Entry point: `orchestrator <command>`

---

## Scenario 1: 验证有效配置

### Preconditions

- 有效的配置文件存在

### Goal

验证有效配置通过验证。

### Steps

1. 创建有效配置:
   ```bash
   cat > /tmp/valid-config.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: "."
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: test-agent
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: test
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   EOF
   ```

2. 验证配置:
   ```bash
   orchestrator manifest validate -f /tmp/valid-config.yaml
   ```

### Expected

- 验证成功，输出 "Manifest is valid"

---

## Scenario 2: 验证无效配置 - 空 workspace

### Preconditions

- 无

### Goal

验证检测到空的 workspace qa_targets 错误。

> Note: Each scenario isolates a single validation error to avoid masking by earlier checks.

### Steps

1. 创建无效配置 (仅 qa_targets 为空，其它字段均有效):
   ```bash
   cat > /tmp/invalid-ws.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: /tmp
     qa_targets: []
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: test-agent
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: test
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   EOF
   ```

2. 验证配置:
   ```bash
   orchestrator manifest validate -f /tmp/invalid-ws.yaml
   ```

### Expected

- 错误信息包含: `qa_targets` 相关错误 (e.g., `qa_targets cannot be empty`)
- 验证失败

---

## Scenario 3: 验证无效配置 - workflow 无 steps

### Preconditions

- 无

### Goal

验证检测到 workflow 无启用步骤的错误。

> Note: `steps: []` is normalized to default steps (all disabled), triggering "no enabled steps".

### Steps

1. 创建无效配置 (仅 workflow steps 为空，其它字段均有效):
   ```bash
   cat > /tmp/invalid-workflow.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: /tmp
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: test-agent
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: test
   spec:
     steps: []
     loop:
       mode: once
   EOF
   ```

2. 验证配置:
   ```bash
   orchestrator manifest validate -f /tmp/invalid-workflow.yaml
   ```

### Expected

- 错误信息包含 workflow steps 相关错误 (e.g., `Workflow must have at least one step` or `has no enabled steps`)

---

## Scenario 4: 验证无效配置 - agent 模板缺失

### Preconditions

- 无

### Goal

验证检测到 workflow 引用不存在的 agent 模板。

> Note: qa_targets must be non-empty to reach template validation.

### Steps

1. 创建无效配置 (workflow 需要 qa 但没有 agent 提供 qa 模板):
   ```bash
   cat > /tmp/invalid-template.yaml << 'EOF'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: /tmp
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: fix-only
   spec:
     capabilities:
       - fix
     command: "echo '{\"confidence\":0.82,\"quality_score\":0.78,\"artifacts\":[{\"kind\":\"code_change\",\"files\":[\"fix-sample.patch\"]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: test
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   EOF
   ```

2. 使用项目级隔离验证配置:
   ```bash
   orchestrator manifest validate -f /tmp/invalid-template.yaml -p "qa-validate-$(date +%s)"
   ```

### Expected

- `manifest validate` reports a **semantic validation error**: the workflow step
  `qa` requires a capability that no agent in the manifest provides.
- Example output:
  ```
  Config build failed
    [config_build_failed] no agent supports capability for step 'qa' used by workflow 'test'
  ```
- Exit code: **1** (validation failure).
- **Note**: `manifest validate` performs both **structural** validation (YAML
  syntax, required fields, type constraints) and **semantic** validation
  (cross-resource checks such as capability-to-agent resolution, env-store
  references, execution profile existence). This is by design: the command
  builds the full active config internally via `build_active_config()` to catch
  integration issues early before `apply`.

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Validation fails with `no agent supports capability` | Workflow step requires a capability not provided by any agent in the manifest | Add an agent with the required capability, or remove/disable the step |
| Validation passes despite missing capability | An agent with the matching capability already exists in the active DB config from a prior `apply` | Use `-p <unique-project>` to isolate validation from residual DB state |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 验证有效配置 | ☐ | | | |
| 2 | 验证无效配置 - 空 workspace | ☐ | | | |
| 3 | 验证无效配置 - workflow 无 steps | ☐ | | | |
| 4 | 验证无效配置 - agent 模板缺失 | ☐ | | | |
