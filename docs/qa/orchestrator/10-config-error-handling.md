# Orchestrator - 配置缺失与 Manifest 错误处理

**Module**: orchestrator
**Scope**: 验证 `init + apply -f` 路径下的配置缺失与错误处理
**Scenarios**: 4
**Priority**: High

---

## Background

Orchestrator 运行时配置存储于 SQLite。`init` 初始化目录、数据库并写入默认配置
（包含 default workspace 和预定义 workflow）。
用户可通过 `apply -f <manifest.yaml>` 导入自定义配置来覆盖或扩展默认配置。

Entry point: `./scripts/orchestrator.sh <command>`

---

## Scenario 1: init 后默认配置已存在，命令可正常执行

### Preconditions

- sqlite 中无配置（空库）

### Goal

验证 `init` 创建默认配置后，依赖配置的命令（如 `task list`）可正常执行。

> **Note**: `init` 会自动创建 default workspace、基本 workflow 和 default agents。
> 因此 `init` 之后即使未执行 `apply`，`task list` 等命令也能正常运行。
> "no manifest" 错误只在 config 表完全为空时出现（例如手动重置数据库但未运行 `init`），
> 但 CLI 入口总会隐式调用 `init`，所以该错误路径对用户不可见。

### Steps

1. 清理并初始化：
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh init
   ```

2. 执行依赖配置的命令（不执行 apply）：
   ```bash
   ./scripts/orchestrator.sh task list
   ```

### Expected

- `task list` 成功执行（退出码 0），因为 `init` 已创建默认配置
- 输出为空列表或包含任务列表，无错误信息
- 无 panic

---

## Scenario 2: init 创建默认配置，apply 可叠加自定义资源

### Preconditions

- sqlite 中无配置

### Goal

验证 `init` 创建默认配置后基本命令可用，`apply -f` 可叠加自定义资源。

### Steps

1. 初始化并验证默认配置可用：
   ```bash
   QA_PROJECT="qa-${USER}-$(date +%Y%m%d%H%M%S)"
   ./scripts/orchestrator.sh qa project reset "${QA_PROJECT}" --keep-config --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   ./scripts/orchestrator.sh qa project create "${QA_PROJECT}" --force
   ./scripts/orchestrator.sh init
   ./scripts/orchestrator.sh workspace list
   ```

2. 导入自定义 manifest 叠加资源：
   ```bash
   ./scripts/orchestrator.sh apply -f fixtures/manifests/bundles/output-formats.yaml
   ```

3. 再次验证：
   ```bash
   ./scripts/orchestrator.sh workspace list
   ./scripts/orchestrator.sh task list
   ```

### Expected

- Step 1 因 init 创建默认配置而成功，workspace list 返回 default workspace
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
| 1 | init 后默认配置已存在，命令可正常执行 | ☐ | | | |
| 2 | init 后必须 apply manifest | ☐ | | | |
| 3 | apply 非法 Manifest 失败 | ☐ | | | |
| 4 | apply 语法损坏文件失败 | ☐ | | | |
