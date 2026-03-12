# FR-020: 自动化 protoc 依赖安装

**优先级**: P0
**状态**: Proposed
**目标**: 零摩擦开发者入门

## 背景与目标

项目使用 gRPC/protobuf 作为控制面通信协议，构建时依赖 `protoc`（Protocol Buffers 编译器）。当前开发者需要手动安装 `protoc` 并确保版本匹配，否则 `cargo build` 在 `prost-build` / `tonic-build` 阶段失败。这对新贡献者构成入门障碍，也导致 CI 环境配置碎片化。

目标：

- 开发者 clone 后直接 `cargo build` 即可成功，无需手动安装 `protoc`。
- CI 环境自动获取正确版本的 `protoc`，无需额外 setup step。
- 锁定 `protoc` 版本，确保所有环境生成一致的代码。

非目标：

- 替换 protobuf 为其他序列化方案。
- 自动安装 Rust 工具链本身。

## 实施方案

### 方案评估

| 方案 | 优点 | 缺点 |
|------|------|------|
| `protobuf-src` crate（编译时从源码构建 protoc） | 零外部依赖，完全自包含 | 首次构建慢（约 2-3 分钟），增加编译时间 |
| `protoc-bin-vendored` crate（内嵌预编译 protoc 二进制） | 快速，无额外编译时间 | 二进制体积增加，支持平台有限 |
| build.rs 自动下载（检测缺失时从 GitHub Releases 下载） | 灵活，可锁定精确版本 | 构建时需要网络，离线构建失败 |
| 开发者文档 + CI setup script | 实现简单 | 未解决核心痛点，仍需手动操作 |

### 推荐方案：`protobuf-src` + 可选环境变量覆盖

1. 在 workspace `build-dependencies` 中引入 `protobuf-src`。
2. `build.rs` 逻辑：若环境变量 `PROTOC` 已设置且指向有效二进制，则使用外部 `protoc`；否则回退到 `protobuf-src` 编译。
3. CI 中可预装 `protoc` 并设置 `PROTOC` 环境变量以跳过编译，加速 CI。
4. 锁定 `protobuf-src` 版本与项目 `.proto` 文件 syntax 版本一致。

### 实施步骤

1. **引入依赖**：`Cargo.toml` 添加 `protobuf-src` build-dependency。
2. **修改 build.rs**：添加 `PROTOC` 环境变量检测逻辑。
3. **更新 CI**：在 CI 中预装 `protoc` 并导出 `PROTOC`，避免每次 CI 重新编译。
4. **更新开发者文档**：说明可选的手动安装路径（性能优化）。
5. **移除现有 protoc 安装文档中的"必须"措辞**，改为"可选优化"。

## CLI / API 影响

无。本 FR 为构建基础设施改进，不涉及用户可见接口变更。

## 关键设计决策与权衡

### `protobuf-src` vs `protoc-bin-vendored`

选择 `protobuf-src` 因其覆盖所有 Rust 支持的平台，无需维护预编译二进制矩阵。首次编译增加约 2-3 分钟的代价可通过 CI 缓存和 `PROTOC` 覆盖机制缓解。

### 环境变量覆盖优先

允许高级用户和 CI 通过 `PROTOC` 环境变量使用自己的 protoc，兼顾"零配置"与"可控性"。

## 风险与缓解

风险：`protobuf-src` 编译时间影响开发者首次构建体验。
缓解：`cargo build` 缓存后后续构建不受影响；文档引导频繁构建的开发者预装 `protoc`。

风险：`protobuf-src` 版本与 `.proto` 文件 syntax 版本不匹配导致生成代码差异。
缓解：CI 中增加 `protoc --version` 断言，锁定 `protobuf-src` 与 proto syntax 版本对应关系。

## 验收标准

- 全新环境（无预装 protoc）下 `cargo build --workspace` 一次成功。
- 设置 `PROTOC=/path/to/protoc` 时使用外部 protoc，不触发 `protobuf-src` 编译。
- CI 构建时间不因本变更增加超过 30 秒（利用缓存或预装 protoc）。
- `cargo test --workspace` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- 开发者文档更新，protoc 安装标注为"可选优化"。
