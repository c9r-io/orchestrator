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

**API Testing Method**: 当 Tauri UI 运行时，调用 `POST /api/validate_config_yaml`

---

## Test Method

### 单元测试 (推荐)

```bash
cd orchestrator/src-tauri
cargo test config_validation --lib
```

预期结果: 14 passed, 0 failed

### API 测试 (需要启动 UI)

```bash
# 启动 Tauri UI
cd orchestrator && npm run tauri:dev

# 在另一个终端调用 API
curl -X POST http://localhost:1420/api/validate_config_yaml \
  -H "Content-Type: application/json" \
  -d '{"yaml": "..."}'
```

---

## Scenario 1: YAML 语法错误预检

### Preconditions

- 无

### Goal

验证 YAML 语法错误能被提前检测，不会导致程序崩溃。

### Steps

1. 发送包含 YAML 语法错误的配置:
   ```bash
   curl -X POST http://localhost:1420/api/validate_config_yaml \
     -H "Content-Type: application/json" \
     -d '{"yaml": "invalid: yaml: content: ["}'
   ```

### Expected

- 返回 `valid: false`
- `errors` 数组包含 YamlSyntaxError
- `errors[0].message` 包含 "YAML syntax error"
- `errors[0].code` 为 "YamlSyntaxError"

---

## Scenario 2: 多错误聚合报告

### Preconditions

- 无

### Goal

验证配置校验返回所有错误而非仅第一个。

### Steps

1. 发送包含多个错误的配置（空 workspaces、agents、workflows、缺失 defaults）:
   ```bash
   curl -X POST http://localhost:1420/api/validate_config_yaml \
     -H "Content-Type: application/json" \
     -d '{
       "yaml": "runner:\n  shell: /bin/bash\nworkspaces: {}\nagents: {}\nworkflows: {}\n"
     }'
   ```

### Expected

- 返回 `valid: false`
- `errors` 数组长度 >= 3
- 包含 "workspaces cannot be empty"
- 包含 "agents cannot be empty"  
- 包含 "workflows cannot be empty"

---

## Scenario 3: 警告与错误分离

### Preconditions

- 无

### Goal

验证路径不存在时返回警告而非错误。

### Steps

1. 发送包含不存在路径的配置（missing_path_is_error: false）:
   ```bash
   curl -X POST http://localhost:1420/api/validate_config_yaml \
     -H "Content-Type: application/json" \
     -d '{
       "yaml": "runner:\n  shell: /bin/bash\nresume:\n  auto: false\ndefaults:\n  project: default\n  workspace: default\n  workflow: basic\nworkspaces:\n  test:\n    root_path: /nonexistent/path/xyz123\n    qa_targets:\n      - docs\n    ticket_dir: tickets\nagents:\n  echo:\n    capabilities:\n      - qa\n    templates:\n      qa: echo test\nworkflows:\n  basic:\n    steps:\n      - id: qa\n        type: qa\n        enabled: true\n"
     }'
   ```

### Expected

- 返回 `valid: true`（警告不影响有效性）
- `warnings` 数组非空
- 包含 PathNotExists 警告
- `warnings[0].suggestion` 包含创建目录的建议

---

## Scenario 4: 路径逃逸检测

### Preconditions

- 无

### Goal

验证路径逃逸尝试被阻止。

### Steps

1. 发送包含路径逃逸的配置（通过 qa_targets 使用 `..`）:
   ```bash
   curl -X POST http://localhost:1420/api/validate_config_yaml \
     -H "Content-Type: application/json" \
     -d '{
       "yaml": "runner:\n  shell: /bin/bash\nresume:\n  auto: false\ndefaults:\n  project: default\n  workspace: default\n  workflow: basic\nworkspaces:\n  test:\n    root_path: .\n    qa_targets:\n      - ../../../etc\n    ticket_dir: tickets\nagents:\n  echo:\n    capabilities:\n      - qa\n    templates:\n      qa: echo test\nworkflows:\n  basic:\n    steps:\n      - id: qa\n        type: qa\n        enabled: true\n"
     }'
   ```

### Expected

- 返回 `valid: false`
- `errors` 包含 PathOutsideWorkspace 错误

---

## Scenario 5: 完整校验报告格式

### Preconditions

- 无

### Goal

验证 API 返回完整的校验报告格式。

### Steps

1. 发送有效配置:
   ```bash
   curl -X POST http://localhost:1420/api/validate_config_yaml \
     -H "Content-Type: application/json" \
     -d '{
       "yaml": "runner:\n  shell: /bin/bash\n  shell_arg: -lc\nresume:\n  auto: false\ndefaults:\n  project: default\n  workspace: default\n  workflow: basic\nworkspaces:\n  default:\n    root_path: .\n    qa_targets:\n      - src\n    ticket_dir: tickets\nagents:\n  echo:\n    capabilities:\n      - qa\n    templates:\n      qa: echo test\nworkflows:\n  basic:\n    steps:\n      - id: qa\n        type: qa\n        enabled: true\n    loop:\n      mode: once\n      guard:\n        enabled: false\n        stop_when_no_unresolved: false\n    finalize:\n      rules: []\n"
     }'
   ```

### Expected

- 返回包含以下字段的 JSON:
  - `valid`: boolean
  - `normalized_yaml`: string (规范化后的 YAML)
  - `errors`: array
  - `warnings`: array
  - `summary`: string (人类可读报告)

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML 语法错误预检 | ☐ | | | |
| 2 | 多错误聚合报告 | ☐ | | | |
| 3 | 警告与错误分离 | ☐ | | | |
| 4 | 路径逃逸检测 | ☐ | | | |
| 5 | 完整校验报告格式 | ☐ | | | |
