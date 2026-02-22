# Orchestrator - 配置验证测试

**Module**: orchestrator
**Scope**: 验证配置验证功能和错误检测
**Scenarios**: 4
**Priority**: High

---

## Background

测试 `config validate` 命令和配置错误检测。

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: 验证有效配置

### Preconditions

- 有效的配置文件存在

### Goal

验证有效配置通过验证。

### Steps

1. 创建有效配置:
   ```bash
   cd orchestrator
   cat > /tmp/valid-config.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: test
   workspaces:
     default:
       root_path: /tmp/test
       qa_targets:
         - docs/qa
       ticket_dir: docs/ticket
   agents:
     test-agent:
       metadata:
         name: test-agent
         description: Test agent
       capabilities:
         - qa
       templates:
         qa: "echo 'test'"
   workflows:
     test:
       steps:
         - id: run_qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: true
       finalize:
         rules: []
   EOF
   ```

2. 验证配置:
   ```bash
   ./scripts/orchestrator.sh config validate /tmp/valid-config.yaml
   ```

### Expected

- 验证成功，输出 "Configuration is valid"

---

## Scenario 2: 验证无效配置 - 空 workspace

### Preconditions

- 无

### Goal

验证检测到空的 workspace 错误。

### Steps

1. 创建无效配置:
   ```bash
   cd orchestrator
   cat > /tmp/invalid-ws.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: test
   workspaces:
     default:
       root_path: ""
       qa_targets: []
       ticket_dir: docs/ticket
   agents: {}
   workflows:
     test:
       steps: []
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ```

2. 验证配置:
   ```bash
   ./scripts/orchestrator.sh config validate /tmp/invalid-ws.yaml
   ```

### Expected

- 错误信息包含: `workspace` 相关错误
- 验证失败

---

## Scenario 3: 验证无效配置 - workflow 无 steps

### Preconditions

- 无

### Goal

验证检测到 workflow 无步骤的错误。

### Steps

1. 创建无效配置:
   ```bash
   cd orchestrator
   cat > /tmp/invalid-workflow.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: test
   workspaces:
     default:
       root_path: /tmp/test
       qa_targets: []
       ticket_dir: docs/ticket
   agents: {}
   workflows:
     test:
       steps: []
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ```

2. 验证配置:
   ```bash
   ./scripts/orchestrator.sh config validate /tmp/invalid-workflow.yaml
   ```

### Expected

- 错误信息: `workflow 'test' must define at least one step` 或类似

---

## Scenario 4: 验证无效配置 - agent 模板缺失

### Preconditions

- 无

### Goal

验证检测到 workflow 引用不存在的 agent 模板。

### Steps

1. 创建无效配置 (workflow 需要 qa 但没有 agent 提供 qa 模板):
   ```bash
   cd orchestrator
   cat > /tmp/invalid-template.yaml << 'EOF'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   defaults:
     workspace: default
     workflow: test
   workspaces:
     default:
       root_path: /tmp/test
       qa_targets: []
       ticket_dir: docs/ticket
   agents:
     fix-only:
       metadata:
         name: fix-only
         description: Fix only
       capabilities:
         - fix
       templates:
         fix: "echo 'fix'"
   workflows:
     test:
       steps:
         - id: run_qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
       finalize:
         rules: []
   EOF
   ```

2. 验证配置:
   ```bash
   ./scripts/orchestrator.sh config validate /tmp/invalid-template.yaml
   ```

### Expected

- 错误信息: `no agent has template for step 'qa' used by workflow 'test'`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 验证有效配置 | ☐ | | | |
| 2 | 验证无效配置 - 空 workspace | ☐ | | | |
| 3 | 验证无效配置 - workflow 无 steps | ☐ | | | |
| 4 | 验证无效配置 - agent 模板缺失 | ☐ | | | |
