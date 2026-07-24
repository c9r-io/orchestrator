# FR-122: CLI、Daemon 与 Tauri 边界层覆盖率治理

## 优先级: P1

## 状态: Proposed

## 背景

历史 FR 审计显示，核心领域 crate 的行覆盖率较强，但用户入口与适配层明显偏低：

- `agent-orchestrator`: 89.83%
- `orchestrator-config`: 95.15%
- `orchestrator-security`: 82.75%
- `orchestrator-collab`: 93.14%
- `orchestratord`: 22.97%
- `orchestrator-cli`: 33.90%
- GUI Rust/Tauri crate: 4.15%

前端 Vitest 行覆盖率已达到 86.96%，但不能替代 Tauri command、gRPC 映射、CLI 参数/输出和真实权限边界测试。当前 Rust `cargo llvm-cov` 报告也没有有效 branch 数据，容易把“行执行过”误当作异常、权限和状态分支均已验证。

## 目标

- 建立可重复、机器可读的分 crate 覆盖基线与非回退门禁。
- 优先补齐安全/状态关键 RPC、CLI 和 Tauri command，而不是追求无差别百分比。
- 建立 Rust branch coverage 的可行方案；工具链不支持时采用明确的分支场景清单替代，禁止把 branch=0 解释为完整覆盖。
- 将历史 FR 的 DD/QA 场景映射到实际可执行测试层。

## 非目标

- 不要求真实 Slack/OAuth、付费模型或公网服务进入普通 PR CI。
- 不以删除代码、忽略文件或测试实现细节的方式提升数字。
- 不一次性为所有展示格式追求 100% 覆盖。

## 需求

### 1. 基线与报告

- 提供统一命令生成 workspace、crate 和关键模块的 line/function/branch 报告。
- 报告区分 core/domain、daemon adapter、CLI、Tauri Rust、React 和 Playwright。
- CI 保存 JSON/LCOV artifact，并与批准基线比较。
- 测试代码、生成代码和不可执行平台代码的排除规则必须书面化。

### 2. 风险导向门禁

- 对 daemon 的 Attention、Handoff、Session、SourceConnection 和 Action Audit 建立 RPC 请求/响应/错误映射测试。
- 对 CLI 的 mutation 参数、机器输出、错误状态码与 UDS/TLS 连接失败建立测试。
- 对 Tauri commands 建立 mock gRPC client 或进程内测试 seam，验证参数映射、RBAC 呈现和错误传播。
- 新增关键 mutation 必须同时具备 success、invalid input、denied、conflict/stale 和 backend unavailable 场景。

### 3. 覆盖率策略

- 初始门禁采用“批准基线不回退 + 变更模块必须增加风险场景”，不设置鼓励投机的全局 100% 目标。
- 为关键边界模块制定分阶段目标，并在 QA 文档记录分母、排除项和达到日期。
- 评估 `cargo llvm-cov` branch 支持的稳定性与跨平台一致性；可用则纳入报告，不可用则输出显式 unsupported 状态及 scenario coverage。

### 4. FR 可追溯性

- 为 FR-095～FR-118 的 DD/QA 建立测试证据索引，标记 unit、integration、shell QA、Playwright、live certification。
- “Closed” 只表示验收证据完备，不自动等价为每个代码文件高覆盖。
- live/manual 场景与普通 CI 场景在索引中分开。

## 验收标准

- [ ] 单一命令可生成机器可读的 workspace、crate 和关键模块覆盖报告
- [ ] CI 对批准基线执行非回退检查，并上传可审计 artifact
- [ ] daemon 五个关键边界均覆盖 success、invalid/denied 和 stale/conflict 类场景
- [ ] CLI 与 Tauri 各至少一条真实 gRPC adapter 垂直测试模板，可复用于后续命令
- [ ] Rust branch coverage 明确显示真实百分比或 `unsupported`，不再显示含义不明的 0
- [ ] FR-095～FR-118 具有 DD/QA→可执行测试类型的证据索引
- [ ] `cargo test --workspace`、Clippy、Vitest、Playwright 核心路径纳入分层验证

## QA 计划

- 对覆盖采集脚本编写 fixture 测试：基线通过、回退失败、unsupported branch。
- 在 Linux/macOS CI 比较 path normalization 与排除规则。
- 对 daemon/CLI/Tauri 测试 seam 分别建立最小垂直样例。
- 审查覆盖提升 diff，确认断言验证领域结果而非只执行代码。

## 风险与缓解

- **CI 时间增加**：按 PR 快速门禁与定期全量报告分层。
- **覆盖率投机**：关键路径使用场景矩阵和 mutation/error 断言补充数字。
- **跨平台 branch 差异**：固定工具版本，平台特有报告独立比较。
- **历史映射维护成本**：闭环 FR 时把证据索引更新纳入治理模板。

## 依赖与参考

- `docs/feature_request/README.md`
- `docs/qa/orchestrator/153-process-console-release-acceptance.md`
- `docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md`

