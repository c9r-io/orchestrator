# Orchestrator - 增强配置校验系统

**Module**: orchestrator
**Scope**: 验证增强的配置校验系统（YAML语法预检、分层校验、错误聚合）
**Scenarios**: 5
**Priority**: High

---

## Background

测试新的 manifest 预检与语义校验能力：
- YAML 语法预检（反序列化前检测）
- 分层校验（语法 + 资源语义）
- 错误/警告聚合报告
- 路径存在性检查（警告 vs 错误）
- 路径安全检查（逃逸检测）

**Primary Testing Method**: CLI 校验 + 资源单元测试

---

## Test Method

### 单元测试 (推荐)

```bash
cd core
cargo test cli_types::tests resource::tests --lib
```

### CLI 配置校验

```bash
# 构建二进制
cd core
cargo build --release

# 使用 CLI 校验配置文件
cd ..
./core/target/release/agent-orchestrator manifest validate -f /tmp/test-config.yaml
```

---

## Scenario 1: YAML 语法错误预检

### Goal

验证 YAML 语法错误能被提前检测，不会导致程序崩溃。

### Steps

1. 创建无效 YAML:
   ```bash
   cat > /tmp/invalid-yaml.yaml << 'YAML'
   invalid: yaml: content: [
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator manifest validate -f /tmp/invalid-yaml.yaml
   ```

### Expected

- 命令返回非零退出码
- 输出包含 YAML 语法错误

---

## Scenario 2: 多错误聚合报告

### Goal

验证配置校验能识别多个资源级错误并逐个报告。

> Note: `manifest validate` requires multi-document YAML with `apiVersion/kind/metadata/spec`.
> Per-document resource-level errors are aggregated; cross-resource semantic errors
> (e.g., empty workspaces) are reported as a single error from `build_active_config`.

### Preconditions

```bash
./scripts/orchestrator.sh init --force
QA_PROJECT="qa-config-enhanced-${USER}-$(date +%Y%m%d%H%M%S)"
./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
rm -rf "workspace/${QA_PROJECT}"
```

### Steps

1. 创建包含多个资源级错误的配置 (每个文档都包含一个校验错误):
   ```bash
   cat > /tmp/multi-error.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: ""
   spec:
     root_path: /tmp
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   ---
   apiVersion: orchestrator.dev/v2
   kind: RuntimePolicy
   metadata:
     name: runtime
   spec:
     runner:
       shell: /bin/bash
       shell_arg: -lc
       policy: allowlist
       executor: shell
       allowed_shells: []
       allowed_shell_args: []
     resume:
       auto: false
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator manifest validate -f /tmp/multi-error.yaml
   ```

### Expected

- 命令返回非零退出码
- 输出包含多个校验错误 (e.g., `metadata.name cannot be empty`, `runner.allowed_shells cannot be empty when policy=allowlist`)

### Troubleshooting

| Symptom | Root Cause | Fix |
|---------|-----------|-----|
| Error: `missing field apiVersion` | Config uses flat format instead of resource format | Use multi-document YAML with `apiVersion/kind/metadata/spec` |

---

## Scenario 3: 路径不存在错误

### Goal

验证不存在路径能被识别并返回错误退出码。

### Steps

1. 创建包含不存在目录的配置:
   ```bash
   cat > /tmp/missing-path.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: /nonexistent/path/xyz123
     qa_targets:
       - docs
     ticket_dir: tickets
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: echo
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: basic
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator manifest validate -f /tmp/missing-path.yaml
   ```

### Expected

- Non-existent path returns an error exit code
- 输出包含路径检查结果

---

## Scenario 4: 路径逃逸检测

### Goal

验证路径逃逸尝试被阻止。

### Steps

1. 创建含路径逃逸的配置:
   ```bash
   cat > /tmp/path-escape.yaml << 'YAML'
   apiVersion: orchestrator.dev/v2
   kind: Workspace
   metadata:
     name: default
   spec:
     root_path: .
     qa_targets:
       - ../../../etc
     ticket_dir: tickets
   ---
   apiVersion: orchestrator.dev/v2
   kind: Agent
   metadata:
     name: echo
   spec:
     capabilities:
       - qa
     command: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[{\"kind\":\"analysis\",\"findings\":[{\"title\":\"qa-sample\",\"description\":\"qa sample\",\"severity\":\"info\"}]}]}'"
   ---
   apiVersion: orchestrator.dev/v2
   kind: Workflow
   metadata:
     name: basic
   spec:
     steps:
       - id: qa
         type: qa
         enabled: true
     loop:
       mode: once
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator manifest validate -f /tmp/path-escape.yaml
   ```

### Expected

- 命令返回非零退出码
- 输出包含路径越界相关错误

---

## Scenario 5: 有效配置规范化输出

### Goal

验证有效配置被接受并可输出规范化结果。

### Steps

1. 使用已有配置:
   ```bash
   ./scripts/orchestrator.sh manifest export -f /tmp/exported-config.yaml
   ./core/target/release/agent-orchestrator manifest validate -f /tmp/exported-config.yaml
   ```

### Expected

- 命令返回 0
- 输出包含 "Manifest is valid"

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML 语法错误预检 | ☐ | | | |
| 2 | 多错误聚合报告 | ☐ | | | |
| 3 | 路径不存在错误 | ☐ | | | |
| 4 | 路径逃逸检测 | ☐ | | | |
| 5 | 有效配置规范化输出 | ☐ | | | |
