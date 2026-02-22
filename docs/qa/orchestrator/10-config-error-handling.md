# Orchestrator - 配置缺失与 Manifest 错误处理

**Module**: orchestrator
**Scope**: 验证 `init + apply -f` 路径下的配置缺失与错误处理
**Scenarios**: 4
**Priority**: High

---

## Background

Orchestrator 运行时配置存储于 SQLite。`init` 只初始化目录和数据库，不会自动写入配置。
配置必须通过 kubectl 风格命令 `apply -f <manifest.yaml>` 导入。

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: 无配置时命令失败并给出修复建议

### Preconditions

- sqlite 中无配置（空库）

### Goal

验证未初始化配置时错误信息包含明确的 `apply -f` 指引。

### Steps

1. 清理数据库：
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh init
   ```

2. 执行依赖配置的命令：
   ```bash
   ./scripts/orchestrator.sh task list
   ```

### Expected

- 输出包含: `orchestrator config is not initialized in sqlite`
- 输出包含: `run 'orchestrator apply -f <manifest.yaml>' first`
- 命令非 0 退出，且无 panic

---

## Scenario 2: init 后必须 apply manifest

### Preconditions

- sqlite 中无配置

### Goal

验证 `init` 不会隐式创建工作区/工作流，必须显式 `apply -f`。

### Steps

1. 初始化并验证失败：
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force
   ./scripts/orchestrator.sh init
   ./scripts/orchestrator.sh workspace list
   ```

2. 导入最小可运行 manifest：
   ```bash
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
   ```

3. 再次验证：
   ```bash
   ./scripts/orchestrator.sh workspace list
   ./scripts/orchestrator.sh task list
   ```

### Expected

- Step 1 因配置缺失失败
- Step 2 成功并输出资源 apply 结果与配置版本
- Step 3 正常返回

---

## Scenario 3: apply 非法 Manifest 失败

### Preconditions

- Orchestrator 已初始化

### Goal

验证 `apply` 对非法 manifest 提供清晰报错。

### Steps

1. 构造非法 manifest（错误 apiVersion）：
   ```bash
   cat > /tmp/invalid-manifest.yaml << 'EOF2'
   apiVersion: wrong.version/v2
   kind: Workspace
   metadata:
     name: bad
   spec:
     root_path: .
     qa_targets:
       - docs/qa
     ticket_dir: fixtures/ticket
   EOF2
   ```

2. 执行 apply：
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/invalid-manifest.yaml
   ```

### Expected

- 命令非 0 退出
- 输出包含 `Invalid apiVersion`
- SQLite 中活动配置不被该非法文件覆盖

---

## Scenario 4: apply 语法损坏文件失败

### Preconditions

- Orchestrator 已初始化

### Goal

验证 YAML 语法损坏时 `apply` 失败且错误可诊断。

### Steps

1. 写入损坏 YAML：
   ```bash
   echo "invalid: yaml: content: [" > /tmp/broken-manifest.yaml
   ```

2. 执行 apply：
   ```bash
   ./scripts/orchestrator.sh apply -f /tmp/broken-manifest.yaml
   ```

### Expected

- 命令非 0 退出
- 输出包含 YAML 解析错误信息
- 无 panic

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 无配置时命令失败并给出修复建议 | ☐ | | | |
| 2 | init 后必须 apply manifest | ☐ | | | |
| 3 | apply 非法 Manifest 失败 | ☐ | | | |
| 4 | apply 语法损坏文件失败 | ☐ | | | |
