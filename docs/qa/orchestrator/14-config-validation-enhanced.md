# Orchestrator - 增强配置校验系统

**Module**: orchestrator
**Scope**: 验证增强的配置校验系统（YAML语法预检、分层校验、错误聚合）
**Scenarios**: 5
**Priority**: High

---

## Background

测试新的 `config_validation` 模块提供的增强校验功能：
- YAML 语法预检（反序列化前检测）
- 分层校验（SyntaxOnly, Schema, Full）
- 错误/警告聚合报告
- 路径存在性检查（警告 vs 错误）
- 路径安全检查（逃逸检测）

**Primary Testing Method**: 单元测试 (CLI: `cargo test config_validation --lib`)

---

## Test Method

### 单元测试 (推荐)

```bash
cd core
cargo test config_validation --lib
```

### CLI 配置校验

```bash
# 构建二进制
cd core
cargo build --release

# 使用 CLI 校验配置文件
cd ..
./core/target/release/agent-orchestrator config validate /tmp/test-config.yaml
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
   ./core/target/release/agent-orchestrator config validate /tmp/invalid-yaml.yaml
   ```

### Expected

- 命令返回非零退出码
- 输出包含 YAML 语法错误

---

## Scenario 2: 多错误聚合报告

### Goal

验证配置校验能识别多个结构性错误并聚合输出。

> Note: Config must be serde-deserializable (all required struct fields present)
> for post-deserialization semantic validation to aggregate errors.
> Maps with `#[serde(default)]` deserialize as empty maps, so empty `workspaces`,
> `agents`, and `workflows` maps pass serde but fail semantic validation.

### Steps

1. 创建包含多个语义错误的配置 (serde 可解析但语义无效):
   ```bash
   cat > /tmp/multi-error.yaml << 'YAML'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     workspace: nonexistent
     workflow: nonexistent
   workspaces: {}
   agents: {}
   workflows: {}
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator config validate /tmp/multi-error.yaml
   ```

### Expected

- 命令返回非零退出码
- 输出包含多个校验错误信息 (e.g., workspaces empty, agents empty, workflows empty, invalid defaults references)

---

## Scenario 3: 路径不存在错误

### Goal

验证不存在路径能被识别并返回错误退出码。

### Steps

1. 创建包含不存在目录的配置:
   ```bash
   cat > /tmp/missing-path.yaml << 'YAML'
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     project: default
     workspace: default
     workflow: basic
   workspaces:
     default:
       root_path: /nonexistent/path/xyz123
       qa_targets: [docs]
       ticket_dir: tickets
   agents:
     echo:
       capabilities: [qa]
       templates:
         qa: "echo test"
   workflows:
     basic:
       steps:
         - id: qa
           type: qa
           enabled: true
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator config validate /tmp/missing-path.yaml
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
   runner:
     shell: /bin/bash
     shell_arg: -lc
   resume:
     auto: false
   defaults:
     project: default
     workspace: default
     workflow: basic
   workspaces:
     default:
       root_path: .
       qa_targets: [../../../etc]
       ticket_dir: tickets
   agents:
     echo:
       capabilities: [qa]
       templates:
         qa: "echo test"
   workflows:
     basic:
       steps:
         - id: qa
           type: qa
           enabled: true
   YAML
   ```
2. 执行:
   ```bash
   ./core/target/release/agent-orchestrator config validate /tmp/path-escape.yaml
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
   ./scripts/orchestrator.sh config export -f /tmp/exported-config.yaml
   ./core/target/release/agent-orchestrator config validate /tmp/exported-config.yaml
   ```

### Expected

- 命令返回 0
- 输出包含 "Configuration is valid"
- 输出包含规范化后的 YAML

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML 语法错误预检 | ☐ | | | |
| 2 | 多错误聚合报告 | ☐ | | | |
| 3 | 路径不存在错误 | ☐ | | | |
| 4 | 路径逃逸检测 | ☐ | | | |
| 5 | 有效配置规范化输出 | ☐ | | | |
