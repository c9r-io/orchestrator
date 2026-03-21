# Design Doc 77: 减少 QA 场景中的不安全操作

## 背景

FR-060 旨在系统性降低 QA 文档中 `self_referential_safe: false` 的比例，使 full-QA 自回归测试能覆盖更多场景。

启动时 139 个 QA 文档中有 114 个（82%）标记为 unsafe，主要原因是 QA 场景依赖 daemon 启停、`cargo build --release`、`orchestrator apply/delete/task create` 等写操作。

## 设计决策

### 核心方法：QA 文档重写为 Unit Test + Code Review 验证

绝大多数 unsafe QA 场景的"不安全操作"并非核心验证点，而是多余的环境搭建/清理步骤。实际验证逻辑已有充分的 unit test 覆盖。

**重写模式**：
1. 识别场景的核心验证点（如"config 校验拒绝无效输入"）
2. 查找对应 unit test（如 `validate_self_referential_safety_errors_*`）
3. 将 QA 步骤改为 `cargo test -p <crate> --lib -- <test_name>` + code review
4. 移除 daemon 启停、`orchestrator apply/delete`、`cargo build --release` 等包装层

### 分类框架

对每个不安全操作，判断属于：
- **QA 设计不合理** — 测试方法过于暴力，可改用安全方式验证 → 重写
- **天然不安全** — daemon 生命周期 / sandbox OS 强制 / mTLS / 全流程 self-bootstrap → 保留 unsafe

### Partial-Safe 机制

通过 `self_referential_safe_scenarios: [S1, S2, ...]` 标注文档中可安全执行的场景子集，使 full-QA prehook 能选择性执行安全场景。

## 实施结果

### 13 次迭代统计

| 迭代 | 转换文档数 | 新增安全场景 | unsafe 文档变化 |
|------|-----------|------------|----------------|
| 1 | 1 (QA-53) | +4 | 114 → 114 |
| 2 | 3 (QA-85/86/91b) | +11 | 114 → 111 |
| 3 | 4 (QA-14/71/101/102) | +17 | 111 → 107 |
| 4 | 10 (QA-95/scenario2-4/QA-10/00/03/04/06/11) | +28 | 107 → 97 |
| 5 | 10 (QA-05/07/09/10x2/20/22/29/36/43) | +40 | 97 → 77 |
| 6 | 10 (QA-30/82/49/13/42/33/37/38/40/88) | +48 | 77 → 68 |
| 7 | 10 (QA-81/76/75/72/70/61/62/78/74/77) | +28 | 68 → 61 |
| 8 | 10 (QA-80/63/18/44/32/104/21/08/92/98) | +34 | 61 → 50 |
| 9 | 10 (QA-66/67/68/69/73/73b/79/20/22/44) | +35 | 50 → 41 |
| 10 | 10 (QA-112/34/90/90b/89/89b/35/drain/105/39) | +52 | — |
| 11 | 10 (QA-97/59/94/16/17/47/48/46/31/94b) | +33 | 50 → 41 |
| 12 | 8 (SB-09/06/QA-74/77/44/53/27/SB-08) | +24 | 41 → 33 |
| 13 | 3 (SB-02/QA-54/QA-99) | +6 | 33 → 33 (partial) |
| **总计** | | **+360** | **114 → 33** |

### 终态分类（33 个天然不安全文档）

| 类别 | 数量 | 技术原因 |
|------|------|---------|
| Daemon 生命周期 | 6 | 测试 daemon 启停/信号/PID guard/socket 连续性 |
| Sandbox OS 强制 | 4 | 测试 macOS sandbox-exec 运行时强制 |
| Control Plane 安全 | 2 | 测试 mTLS / gRPC rate limiting |
| Self-Bootstrap 全流程 | 2 | 测试完整 self-bootstrap pipeline |
| Self-Bootstrap 生存 | 2 | 测试 binary checkpoint / cycle2 validation |
| Runtime Task 执行 | 5 | 测试 end-to-end task 执行 |
| CLI + Daemon 依赖 | 3 | 测试需 gRPC daemon 的 CLI 命令 |
| Smoke 测试 | 2 | 冒烟测试需 daemon 启动 + binary 构建 |
| Partial-Safe | 7 | 已有安全场景标注，剩余场景天然不安全 |

## 关键约束

- 零代码变更（除 1 处 pre-existing clippy lint 修复）— 纯 QA 文档重写
- 零 test 回归 — 所有 409 个 unit test 始终通过
- 零 daemon 被 QA agent 意外 kill
