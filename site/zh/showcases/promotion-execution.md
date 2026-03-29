# 项目更新分享内容自动生成与分发执行计划

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

本文档是 orchestrator 的第 4 类 showcase：**项目更新分享** — 自动化内容创建与多平台分发。与前三类 showcase（自举、自进化、全量 QA）不同，本工作流展示 orchestrator 处理**面向外部的沟通任务**的能力，而非 SDLC 内部循环。

适用场景：项目发布重要功能后的对外说明、定期周报分享、里程碑达成后的内容分发。

---

## 1. 任务目标

> 课题名称：`项目更新分享内容自动生成`
>
> 背景：
> orchestrator 作为 AI 原生 SDLC 自动化工具，需要定期向技术社区分享项目进展与构建经验。
> 手工为多个平台整理这些内容耗时且容易遗漏，适合用 orchestrator 自身来编排自动化。
>
> 本轮任务目标：
> 收集最近的项目变更，由 AI 分析其中真正值得分享的部分，生成 Dev.to / Hashnode /
> Twitter / LinkedIn / HN 等平台的内容草稿；对支持安全 API 的平台执行草稿发布，其余平台保留人工审核稿。
>
> 约束：
> 1. 不修改项目代码，仅生成对外分享内容。
> 2. 通过 WorkflowStore 跟踪已处理内容，避免重复围绕相同提交生成更新。
> 3. HN / Reddit 仅生成草稿，需人工审核后手动提交。
> 4. Dev.to 发布为 `published: false` 草稿状态，需人工确认后发布。

### 1.1 预期产出

由 orchestrator 自主产出：

1. `docs/promotion/drafts/` 下的多平台分享草稿（JSON 格式，含 title/body/metadata）。
2. Dev.to 上的草稿文章（如配置了 `DEVTO_API_KEY`）。
3. WorkflowStore 中的发布记录（`last_published_sha` + 日期索引）。

### 1.2 执行链路

```text
gather_updates(command) → analyze_highlights(agent) → generate_content(agent,item×N) → save_drafts(command,item×N) → publish(command,item×N) → track_results(command) → loop_guard
```

### 1.3 非目标

- 不自动发布到 Hacker News 或 Reddit（无安全 API，且反垃圾机制严格）。
- 不生成视频、播客等非文本内容。
- 不评估传播效果（阅读量、点赞数等）—— 可作为后续迭代。

---

## 2. 前置条件

### 2.1 平台 API Keys（可选）

| 平台 | 获取方式 | 环境变量 | 是否必须 |
|------|---------|---------|---------|
| Dev.to | https://dev.to/settings/extensions | `DEVTO_API_KEY` | 否（无 key 仅生成草稿） |

未配置 API key 时，所有平台均仅生成草稿，不执行发布。

### 2.2 Claude API Credits

本工作流使用 2 个 agent 步骤（`analyze_highlights` + `generate_content` × 平台数），
均使用 claude-sonnet 模型。预计单次执行消耗约 5-10 次 sonnet 调用。

---

## 3. 执行步骤

### 3.1 构建并启动 daemon

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory

cargo build --release -p orchestratord -p orchestrator-cli

# 启动 daemon（如未运行）
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord.log 2>&1 &

# 验证 daemon 运行
ps aux | grep orchestratord | grep -v grep
```

### 3.2 加载资源

```bash
# 如有旧的 promotion project，先清理
orchestrator delete project/promotion --force

orchestrator init

# 加载 secrets（如有 Dev.to API key，先 export DEVTO_API_KEY=xxx）
orchestrator apply -f your-secrets.yaml           --project promotion

# 加载主 workflow
orchestrator apply -f docs/workflow/promotion.yaml --project promotion
```

### 3.3 验证资源已加载

```bash
orchestrator get workspaces --project promotion -o json
orchestrator get agents --project promotion -o json
```

### 3.4 创建任务（手动执行）

```bash
orchestrator task create \
  -n "promotion-weekly" \
  -w promotion -W promotion \
  --project promotion \
  -g "收集最近项目更新，分析其中真正值得分享的内容，为 Dev.to/Hashnode/Twitter/LinkedIn/HN 生成对外草稿"
```

记录返回的 `<task_id>`。任务会立即被 worker 认领并开始执行。

### 3.5 启用定时触发（可选）

```bash
# 启用每周一 10:00 UTC 自动触发
orchestrator trigger resume weekly-promotion --project promotion
```

---

## 4. 监控方法

### 4.1 状态监控

```bash
orchestrator task list --project promotion
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

重点观察：

1. `gather_updates` 是否成功收集 git log
2. `analyze_highlights` 是否输出有效 JSON 并生成 platform items
3. `generate_content` 是否为每个平台生成内容
4. `save_drafts` 是否将草稿写入文件系统
5. `publish` 是否仅对 `api_publishable=true` 的平台执行
6. `track_results` 是否更新 WorkflowStore

### 4.2 日志监控

```bash
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

### 4.3 产出验证

```bash
# 检查草稿文件
ls -la docs/promotion/drafts/

# 查看草稿内容
cat docs/promotion/drafts/*.json | python3 -m json.tool

# 检查 WorkflowStore
orchestrator store list promotion --project promotion
orchestrator store get promotion last_published_sha --project promotion
```

---

## 5. 关键检查点

### 5.1 Gather Updates 检查点

确认 git log 输出包含有意义的提交信息：

- [ ] 输出非空
- [ ] 如有 `last_published_sha`，仅包含新提交
- [ ] 输出末尾包含当前 HEAD SHA

### 5.2 Analyze Highlights 检查点

确认 AI 分析结果合理：

- [ ] 输出为有效 JSON
- [ ] `highlights` 数组有 1-3 个条目
- [ ] `platforms` 数组有 3-5 个条目
- [ ] 每个 platform 的 `api_publishable` 字段正确，仅在存在安全草稿发布路径时为 true
- [ ] `generate_items` post-action 成功生成 items

### 5.3 Generate Content 检查点

确认内容质量：

- [ ] 每个平台生成了格式正确的内容
- [ ] Dev.to/Hashnode 内容为完整博文（800+ 字）
- [ ] Twitter 内容为 3-7 条推文线程
- [ ] HN 内容语气克制、无营销话术、无强行竞品比较
- [ ] 所有内容包含项目 URL

### 5.4 Publish 检查点

- [ ] `publish` 步骤仅对 `api_publishable=true` 的 items 执行
- [ ] 如配置了 `DEVTO_API_KEY`，Dev.to API 返回成功
- [ ] 未配置 API key 时，步骤优雅降级（提示 draft saved）

---

## 6. 成功判定

当以下条件同时成立，可判定本轮更新分享执行完成：

1. orchestrator 完整跑完 promotion 流程，loop_guard 正常收口。
2. `docs/promotion/drafts/` 下至少有 3 个平台的草稿文件。
3. WorkflowStore 中 `last_published_sha` 已更新为当前 HEAD。
4. 如配置了 `DEVTO_API_KEY`，Dev.to Dashboard 中可见草稿文章。
5. workflow 状态为 completed，无异常 failure。

---

## 7. 异常处理

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| 无新变更可分享 | `analyze_highlights` 返回空 `highlights` | 正常终止，不生成内容 |
| API key 未配置 | `publish` 步骤输出 "not set" | 忽略发布，草稿已保存 |
| AI 生成无效 JSON | `generate_content` captures 失败 | 检查 agent 日志，调整 prompt |
| Dev.to API 返回 401 | curl 输出 Unauthorized | 检查 `DEVTO_API_KEY` 是否有效 |
| Dev.to API 返回 422 | curl 输出 Unprocessable | 检查文章格式（title 必填，tags 限制等） |
| WorkflowStore 写入失败 | `track_results` 报错 | 手动 `orchestrator store put` 补录 |
| agent 长时间无输出 | `task watch` 步骤超时 | 检查 Claude API 状态和网络连接 |

---

## 8. 人工角色边界

本计划中，人工角色明确限定为：

1. **一次性配置**：设置平台 API keys。
2. **启动**：创建任务或启用 cron trigger。
3. **监控**：观察执行状态和产出质量。
4. **审核发布**：
   - Dev.to：在 Dashboard 中将草稿从 unpublished 改为 published。
   - Hacker News：阅读草稿，手动在 HN 提交 Show HN。
   - Twitter：阅读推文线程草稿，手动发布。
   - LinkedIn：阅读短文草稿，手动发布。
5. **异常处理**：在内容质量不达标时中断并调整。

人工不提前替 orchestrator 写宣传文案，不预设平台选择。内容角度由 AI 分析项目变更后自主决定，人工只审核最终产出。
