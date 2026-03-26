# Design Doc 90: Workflow Template Library

## Origin

FR-077 — Workflow 模板库：常见 SDLC 自动化场景预设

## Problem

新用户初次使用 orchestrator 需要从零编写 workflow manifest，学习成本高。现有 showcase 都是复杂的、绑定 orchestrator 自身项目的生产级 workflow，缺少面向任意项目的入门级模板。

## Design Decisions

### 1. 合并 examples 与 showcases

FR 原始提案建议新建 `examples/` 目录。经评估，项目已有 `docs/showcases/` + `docs/workflow/` 结构完善，新增目录会导致维护冗余。最终方案：在现有体系中增加入门级模板，workflow YAML 放 `docs/workflow/`，文档放 `docs/showcases/`。

### 2. Echo Agent 策略

所有模板使用 echo agent（`echo '{...}'`），零 API 成本即可运行。每个 showcase 文档提供替换为真实 agent 的指南（Claude Code、OpenCode 等）。

### 3. 渐进复杂度设计

5 个模板按复杂度递增，逐步引入新资源类型：

| 模板 | 资源 | 新增概念 |
|------|------|---------|
| hello-world | Workspace + Agent + Workflow | 最小集 |
| qa-loop | + 多 Agent + 多步骤 | capability 匹配、ticket_scan |
| plan-execute | + StepTemplate | prompt 变量、Agent 解耦 |
| deployment-pipeline | + ExecutionProfile | 沙箱隔离、safety 熔断 |
| scheduled-scan | + Trigger | cron 调度、自动任务创建 |

### 4. 文档站集成

在 VitePress 文档站的 Showcases 侧边栏顶部新增 "Templates" / "模板" 分组，EN/ZH 同步。导航入口默认指向 Hello World 模板（最友好的入口）。

### 5. CLI init --template 延后

FR 标记 CLI 模板命令为可选。当前通过 `orchestrator apply -f docs/workflow/<name>.yaml --project <name>` 即可完成相同功能，无需新增 CLI 命令。

## Files

### Workflow YAML (5)
- `docs/workflow/hello-world.yaml`
- `docs/workflow/qa-loop.yaml`
- `docs/workflow/plan-execute.yaml`
- `docs/workflow/deployment-pipeline.yaml`
- `docs/workflow/scheduled-scan.yaml`

### Showcase Docs (5)
- `docs/showcases/hello-world.md`
- `docs/showcases/qa-loop.md`
- `docs/showcases/plan-execute.md`
- `docs/showcases/deployment-pipeline.md`
- `docs/showcases/scheduled-scan.md`

### Doc Site Pages (10)
- `site/en/showcases/{hello-world,qa-loop,plan-execute,deployment-pipeline,scheduled-scan}.md`
- `site/zh/showcases/{hello-world,qa-loop,plan-execute,deployment-pipeline,scheduled-scan}.md`

### Config
- `site/.vitepress/config.ts` — sidebar Templates group
