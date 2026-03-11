# FR-009: 数据库迁移框架与持久化边界治理

## 1. 背景

Orchestrator 以 SQLite 作为本地持久化基座，运行期需要同时承载任务状态、事件审计、会话状态、workflow store 以及配置演进等多类数据。  
在 FR-009 提出时，数据库治理存在两个核心问题：

1. `core/src/db.rs` 同时承担连接配置、schema 初始化、迁移调度和运行时 helper，职责耦合严重。
2. 迁移兼容主要依赖公共 `ensure_column` + `PRAGMA table_info` 的手工补列模式，演进边界不清晰。

## 2. 目标

FR-009 的治理目标调整为分阶段落地，而不是一次性替换为全量 ORM：

1. 建立独立的 persistence 基础设施层，解耦 schema/bootstrap、SQLite 连接、repository 边界。
2. 从公共入口移除 `ensure_column`，避免业务代码继续依赖“运行时动态补列”。
3. 先把最明显的高耦合散点收口到 repository trait，再逐步治理其余写路径。
4. 保持现有 SQLite 运行模型和外部 CLI 行为兼容，避免大规模回归。

## 3. 当前实现对齐

### 3.1 已完成（Phase 1）

本轮实现已经完成以下治理动作：

1. 新增 `core/src/persistence/` 基础设施层：
   - `schema.rs`：引入 `PersistenceBootstrap` 和 `SchemaStatus`
   - `sqlite.rs`：集中连接打开与 SQLite pragma 配置
   - `repository/`：新增 `SessionRepository`、`WorkflowStoreRepository` 及其 SQLite 实现
2. `core/src/db.rs` 已降级为兼容包装层：
   - `open_conn` / `configure_conn` 代理到 `persistence::sqlite`
   - `init_schema` 统一代理到 `PersistenceBootstrap::ensure_current`
3. 公共 `ensure_column` 已移除：
   - 业务模块不能再通过 `crate::db::ensure_column` 扩展 schema
   - 兼容性补列逻辑仅保留在 `core/src/migration.rs` 内部私有 helper
4. 两条运行期 SQLite 热点已收口到 repository：
   - `core/src/session_store.rs` 异步封装改为委托 `SessionRepository`
   - `core/src/store/local.rs` 改为委托 `WorkflowStoreRepository`
5. 启动链路已切换到新的 schema bootstrap：
   - `core/src/service/bootstrap.rs` 使用 `PersistenceBootstrap::ensure_current`

### 3.2 尚未完成（后续阶段）

以下目标仍属于 FR-009 后续阶段，不应误判为已完成：

1. 尚未引入 `sqlx`/`sqlx-cli` 或 `SeaORM`，迁移仍由现有 `migration.rs` 驱动。
2. `db_write.rs`、部分 scheduler/query/config 持久化路径仍保留手写 SQL。
3. 尚未提供独立的 `db status` / `db rollback --to <version>` 运维命令。
4. 尚未把全部 CRUD 汇聚到统一 persistence facade。

## 4. 治理结论

FR-009 当前状态应定义为：

- **状态**：部分完成，Phase 1 已交付
- **架构结论**：采用“分阶段治理”，先做持久化边界和 schema bootstrap 收口，再逐步治理迁移框架与剩余 repository 化
- **技术结论**：当前代码已经为后续 `sqlx` 迁移治理预留清晰入口，但并未在本轮引入 `sqlx`

## 5. 验收更新

### 5.1 本轮已满足

1. schema 初始化已有唯一入口：`PersistenceBootstrap::ensure_current`
2. 公共 `ensure_column` 已退出业务可调用面
3. session store 与 local workflow store 已通过 repository trait 解耦
4. 全量单元测试与集成测试通过，验证现有行为未回归

### 5.2 仍保留为 FR-009 后续验收项

1. 将迁移执行切换到独立 migration 目录与更成熟的迁移治理栈
2. 为剩余任务主路径写操作建立统一 repository/facade
3. 增加显式 schema 状态查询与离线回滚运维能力
4. 为历史数据库升级场景补充更完整的样本库兼容验证

## 6. 关联文档

- 设计文档：`docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md`
- QA 文档：`docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md`
