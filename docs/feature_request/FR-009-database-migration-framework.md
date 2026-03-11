# FR-009: 数据库迁移治理与持久化边界收敛

## 1. 背景

Orchestrator 以 SQLite 作为本地持久化基座，运行期同时承载任务状态、事件审计、会话状态、workflow store、配置版本与控制面审计等多类数据。  
FR-009 的原始诉求是“引入安全的 ORM 与完备的数据库迁移治理框架”，但仓库实际演进已经表明，真正需要先治理的是边界，而不是先替换技术栈。

FR-009 提出时，数据库治理存在三类结构性问题：

1. `core/src/db.rs` 同时承担连接配置、schema 初始化、迁移调度和运行时 helper，职责耦合严重。
2. 迁移兼容曾依赖公共 `ensure_column` + `PRAGMA table_info` 的手工补列模式，schema 演进边界不清晰。
3. 运行期读写路径分散在 `db.rs`、`db_write.rs`、scheduler、config persistence 等多个模块，缺少稳定的 persistence seam。

## 2. 治理目标

FR-009 调整为分阶段治理需求，优先解决长期可维护性与演进边界问题，而不是一次性导入全量 ORM：

1. 建立独立的 persistence 基础设施层，解耦 schema bootstrap、SQLite 连接与 repository 边界。
2. 将 schema 演进严格收口到 migration 层，禁止业务模块继续依赖运行时动态补列。
3. 分批把高价值的运行期读写路径收口到 repository / facade，而不是保留散落 SQL。
4. 为 schema 状态查询、迁移可观测性、历史库升级验证建立明确的运维与测试闭环。
5. 保持现有 SQLite 运行模型、外部 CLI/gRPC 行为与单机部署模型兼容。

## 3. 当前状态

### 3.1 已完成：Phase 1 持久化基础设施解耦

当前仓库已经完成以下治理动作：

1. 新增 `core/src/persistence/` 基础设施层：
   - `schema.rs`：提供 `PersistenceBootstrap` 与 `SchemaStatus`
   - `sqlite.rs`：集中连接打开与 SQLite pragma 配置
   - `repository/`：提供 `SessionRepository`、`WorkflowStoreRepository` 及 SQLite 实现
2. `core/src/db.rs` 已降级为兼容 facade：
   - `open_conn` / `configure_conn` 代理到 `persistence::sqlite`
   - `init_schema` 统一代理到 `PersistenceBootstrap::ensure_current`
3. 公共 `ensure_column` 已移除：
   - 业务模块不能再通过 `crate::db::ensure_column` 扩展 schema
   - 兼容性补列逻辑仅保留在 `core/src/persistence/migration_steps.rs` 内部私有 helper
4. 两条运行期热点已收口到 repository：
   - `core/src/session_store.rs` 异步封装委托 `SessionRepository`
   - `core/src/store/local.rs` 委托 `WorkflowStoreRepository`
5. 启动链路已切换到新的 schema bootstrap：
   - `core/src/service/bootstrap.rs` 使用 `PersistenceBootstrap::ensure_current`

### 3.2 尚未完成：FR-009 仍然开放

以下目标仍属于 FR-009 后续阶段：

1. `db_write.rs` 与部分 config persistence 仍保留手写 SQL 与直接连接访问。
2. 历史数据库样本升级验证尚未建立为固定 fixture。
3. 尚未决定是否需要引入 `sqlx`、`SeaORM` 等替代迁移栈；当前代码仍基于 `rusqlite`。

## 4. 约束与结论

FR-009 的治理边界明确如下：

- **状态**：In Progress，Phase 1 已完成，Phase 2-4 未完成。
- **架构结论**：采用“先边界、后迁移内核、再运维”的分阶段治理路线。
- **技术结论**：ORM 不是本 FR 的必选前提，`rusqlite + SQLite` 仍是当前基线；后续若替换底层栈，应建立在清晰的 persistence seam 之上。
- **兼容性结论**：不接受 flag-day 式的大爆炸改造；所有阶段都必须保持现有 CLI/gRPC 合约和已有库文件可升级。

## 5. 分阶段治理计划

### Phase 1：Persistence Bootstrap And Initial Repository Boundaries

- 状态：已交付
- 目标：建立 `persistence/` 作为 schema/bootstrap/SQLite 基础设施入口，移除公共 `ensure_column`，收口 session 与 workflow store 两条热点路径
- 设计文档：`docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md`
- QA 文档：`docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md`

### Phase 2：Migration Kernel Split And Schema Governance

- 状态：进行中
- 目标：
  - 将 `core/src/migration.rs` 的单文件迁移实现拆分为 catalog / runner / status / steps
  - 保留 `rusqlite` 执行模型，不在本阶段引入 ORM
  - 建立唯一迁移注册入口，禁止继续向旧单文件追加实现
  - 让 `SchemaStatus` 成为统一的 schema 只读视图
  - 提供只读 `db status` / `db migrations list` 运维入口
- 当前已完成：
  - `core/src/persistence/migration.rs` 已拥有 `Migration`、registered catalog、status、runner、applied summary
  - `core/src/persistence/migration_steps.rs` 已承载 migration step 实现，`core/src/migration.rs` 已退为兼容 facade + 测试宿主
  - CLI / daemon / core 已支持 `orchestrator db status` 与 `orchestrator db migrations list`
  - `SchedulerRepository` 已落地，`core/src/scheduler_service.rs` 不再直接持有 pending/claim/count SQL
- 设计文档：`docs/design_doc/orchestrator/26-database-migration-kernel-and-repository-governance.md`
- QA 文档：`docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md`

### Phase 3：Repository Expansion For Core Runtime Paths

- 状态：待实施
- 目标：
  - 优先治理 `db_write.rs`、scheduler 读写、config persistence
  - 以业务聚合为边界建立 `TaskRepository`、`SchedulerRepository`、`ConfigRepository`
  - 停止向 `core/src/db.rs` 新增业务查询或写入 helper
  - 彻底禁止运行时代码中的动态 DDL / 补列逻辑

### Phase 4：Operational Visibility And Historical Upgrade Validation

- 状态：待实施
- 目标：
  - 增加 `orchestrator db status`、`orchestrator db migrations list`
  - 补齐迁移可观测性与失败定位信息
  - 建立空库、旧库、半升级库、最新库的升级验证样本
  - 采用“前向迁移 + 备份恢复”的默认回滚策略，不将通用 down migration 作为默认目标

## 6. 后续阶段的接口约束

后续阶段落地时，需遵守以下对外与对内接口规则：

1. 不变更现有 gRPC/CLI 主业务接口。
2. `SchemaStatus` 继续作为 schema 状态查询的唯一只读视图。
3. 新增迁移元数据与执行摘要时，应以内聚类型暴露，例如：
   - `MigrationDescriptor`
   - `AppliedMigrationSummary`
4. 新迁移实现文件不得继续追加到 `core/src/migration.rs`；旧文件仅允许保留兼容转发。
5. 所有 schema 演进必须通过 migration 层完成，运行时代码不得做动态 schema 修补。

## 7. 验收更新

### 7.1 已满足

1. schema 初始化已有唯一入口：`PersistenceBootstrap::ensure_current`
2. 公共 `ensure_column` 已退出业务可调用面
3. session store 与 local workflow store 已通过 repository trait 解耦
4. migration catalog / runner / status 已前移到 `core/src/persistence/migration.rs`
5. CLI 已提供只读 `db status` / `db migrations list`
6. 相关回归测试已覆盖 Phase 1 与当前 Phase 2 已落地能力

### 7.2 FR-009 总体验收条件

FR-009 仅在以下条件全部满足后方可关闭：

1. 迁移内核已从单文件实现拆分为清晰的 persistence migration 子域
2. 任务主路径、scheduler、config persistence 的核心读写已通过 repository/facade 收口
3. CLI 已提供只读 schema/migration 状态查询能力
4. 历史数据库升级验证具备可重复执行的样本与 QA 文档
5. `docs/feature_request/README.md`、对应 design doc 与 QA doc 对 FR 状态描述一致

## 8. 关联文档

- 架构基线：`docs/architecture.md`
- Phase 1 设计文档：`docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md`
- Phase 1 QA 文档：`docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md`
- Phase 2 设计文档：`docs/design_doc/orchestrator/26-database-migration-kernel-and-repository-governance.md`
- Phase 2 QA 文档：`docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md`
