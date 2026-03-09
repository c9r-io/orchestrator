# FR-006: 彻底消除全局设定与实现纯粹的 Project-Only 架构

## 调查背景

针对开发者提到的“`manifest_validate` 调用 `validate_manifests()`，该函数从 DB 加载全局 raw config”的说法，进行深入的代码分析，确认系统中目前的全局/项目的二元性设计。

## 调查结论

**结论：开发者的说法是准确的，系统中目前仍然存在明显的“全局/项目”二元性设计。** 目前所有项目的数据和全局配置依然被打包在一个全局的单例数据结构 `OrchestratorConfig` 中，并通过单一数据库记录进行存取。

## 详细技术分析

### 1. `validate_manifests()` 函数的实际行为
在 `core/src/service/system.rs` 中，`validate_manifests` 在做 YAML 解析后，确实会执行以下操作：
```rust
let mut merged_config = crate::config_load::load_raw_config_from_db(&state.db_path)?
    .map(|(cfg, _, _)| cfg)
    .unwrap_or_default();
```
这里的 `load_raw_config_from_db` 会从 SQLite 的 `orchestrator_config` 表中读取 `id = 1` 的整条记录（包含所有项目、所有资源），并反序列化为全局的 `OrchestratorConfig`。

### 2. `OrchestratorConfig` 结构体：仍包含全局字段
查看 `core/src/config/mod.rs` 中的定义，`OrchestratorConfig` 作为整个系统的唯一配置树，不仅包含了以项目划分的资源，还在**顶层保留了多个全局范围的设定**：

```rust
pub struct OrchestratorConfig {
    // ⬇️ 全局设定（与具体 Project 无关）
    pub runner: RunnerConfig,
    pub resume: ResumeConfig,
    pub observability: ObservabilityConfig,
    
    // ⬇️ 项目范围设定
    pub projects: HashMap<String, ProjectConfig>,
    
    // ⬇️ 混合或全局资源存储
    pub resource_meta: ResourceMetadataStore,
    pub custom_resource_definitions: HashMap<String, CustomResourceDefinition>,
    pub custom_resources: HashMap<String, CustomResource>,
    pub resource_store: ResourceStore,
}
```

### 3. `ResourceStore` (统一资源存储) 的二元性
在 `core/src/crd/store.rs` 中，`ResourceStore` 负责存储所有声明式资源（内置与自定义 CRD）。其底层存储键 `storage_key` 的生成逻辑预留了全局资源的支持：
```rust
fn storage_key(kind: &str, metadata: &crate::cli_types::ResourceMetadata) -> String {
    match metadata.project.as_deref() {
        // 如果指定了 project，则是 project-scoped (例如 "Agent/proj1/my-agent")
        Some(project) if !project.trim().is_empty() => {
            format!("{}/{}/{}", kind, project, metadata.name)
        }
        // 如果没有 project，则是 global (例如 "RuntimePolicy/default")
        _ => format!("{}/{}", kind, metadata.name),
    }
}
```
这表明底层资源引擎本身依然“原生”支持无 Project 归属的全局资源。

### 4. 数据库层面的单体设计
在 `core/src/config_load/persist.rs` 中：
整个系统的所有配置（所有 Project 的所有 Agent、Workflow 以及全局设置等）都被序列化为一个巨大的 JSON 字符串 (`config_json`)，保存在表中的固定 `id = 1` 行。任何一个项目中一个小资源的修改，都会导致这整个全局 JSON 的整体重新读写。

## 需要的重构与建议

在当前架构下，逻辑、存储与资源管理层面都未能实现纯粹的 Project-Only 隔离。要达到目标，建议进行以下维度的重构：

1. **打破单例结构**：将 `runner`, `resume`, `observability` 等配置下放到 `ProjectConfig` 当中，或者作为启动参数/环境变量，而不是作为系统热配置的一部分。
2. **重构数据库存储**：不应该再有单行的全局 `config_json`，而是每个 Resource 独立一行，并以 `project_id` 作为表的外键或复合主键；或者至少按 Project 进行序列化存储，避免大 JSON 单点瓶颈与耦合。
3. **强制隔离资源**：修改 `ResourceStore`，取消全局资源的 Key 构造，在所有资源操作的入口处强制把 `project_id` 作为必填参数，从源头上杜绝全局资源的产生。
