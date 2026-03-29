# AI-Native SDLC Skills 初始化执行计划

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

## 1. 目标

为当前项目初始化 AI-native SDLC skills。Agent 分析项目的语言、框架和目录结构，从 skill 模板中选择适合的 skills，特化后安装到 `.claude/skills/`。

## 2. Skill 模板位置

模板安装在 `~/.orchestratord/skill-templates/`，分三个类别：

```
skill-templates/
├── generic/              # 通用（任何项目）
│   ├── performance-testing/
│
│   └── project-bootstrap/
├── framework/            # 框架相关（根据项目技术栈选择）
│   ├── align-tests/
│   ├── deploy-gh-k8s/
│   ├── e2e-testing/
│   ├── grpc-regression/
│   ├── ops/
│   ├── project-readiness/
│   ├── reset-local-env/
│   ├── rust-conventions/
│   ├── test-authoring/
│   └── test-coverage/
└── sdlc-patterns/        # SDLC 模式（适合需要治理流程的项目）
    ├── fr-governance/
    ├── qa-testing/
    ├── ticket-fix/
    ├── qa-doc-gen/
    └── security-test-doc-gen/
```

## 3. 执行流程

### 3.1 分析项目

Agent 应首先检查：

```bash
# 检查语言和框架
ls Cargo.toml 2>/dev/null && echo "Rust project"
ls package.json 2>/dev/null && echo "Node.js project"
ls go.mod 2>/dev/null && echo "Go project"
ls docker-compose.yml docker/docker-compose.yml 2>/dev/null && echo "Docker Compose found"
ls k8s/ deploy/ 2>/dev/null && echo "Kubernetes found"
ls .github/workflows/ 2>/dev/null && echo "GitHub Actions found"
ls docs/qa/ 2>/dev/null && echo "QA docs found"
```

### 3.2 选择 Skills

根据分析结果，Agent 决定安装哪些 skills：

| 条件 | 安装的 Skills |
|------|---------------|
| 任何项目 | `performance-testing` |
| 有 `Cargo.toml` | `rust-conventions`, `align-tests`, `test-coverage`, `test-authoring` |
| 有 `package.json` | `e2e-testing`, `test-authoring` |
| 有 `docker-compose.yml` | `ops`, `reset-local-env` |
| 有 `k8s/` 或 `deploy/` | `deploy-gh-k8s`, `project-readiness` |
| 有 `.github/workflows/` | `project-readiness` |
| 有 `docs/qa/` | `qa-testing`, `ticket-fix`, `qa-doc-gen` |
| 有 `docs/feature_request/` | `fr-governance` |
| 有 `docs/security/` | `security-test-doc-gen` |

### 3.3 特化模板

对每个选中的 skill：

1. 从 `~/.orchestratord/skill-templates/<category>/<skill>/` 复制到 `.claude/skills/<skill>/`
2. 读取模板中的 `SKILL.md`
3. 根据项目实际情况替换占位符：
   - `<project-root>` → 实际项目根路径
   - `core/` → 实际的后端源码目录
   - `portal/` → 实际的前端目录
   - `docker/docker-compose.yml` → 实际的 compose 文件路径
   - `docs/qa/<project>/` → 实际的 QA 文档目录

### 3.4 验证

```bash
# 确认 skills 已安装
ls .claude/skills/

# 每个 skill 都有 SKILL.md
for d in .claude/skills/*/; do
  [[ -f "$d/SKILL.md" ]] && echo "OK: $d" || echo "MISSING: $d/SKILL.md"
done
```

## 4. 注意事项

- Agent 应只安装与当前项目相关的 skills，不要全部安装
- 如果模板不存在（`~/.orchestratord/skill-templates/` 为空），提示用户运行 `install.sh` 或从 GitHub Release 下载
- 已存在的 `.claude/skills/` 不应被覆盖 — 跳过已有的 skill
- 特化时保持 SKILL.md 的 frontmatter 格式（`---` 分隔的 name/description）
