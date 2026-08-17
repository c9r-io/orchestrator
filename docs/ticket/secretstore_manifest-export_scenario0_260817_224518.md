# `manifest export` 以明文输出 SecretStore 的值

**Status**: FAILED
**发现于**: 2026-08-17，FR-171 治理的 step 0（不是来自某个编号 QA 场景 —— `docs/qa`
下没有覆盖 `manifest export` 输出内容的场景，这本身是本 ticket 的一部分）
**树**: `c6dbc5d7`

## 测试内容

`orchestrator manifest export` 的输出是否对 SecretStore 的 `spec.data` 值脱敏。

## 预期结果

值被替换为 `secret_store_crypto::ENCRYPTED_PLACEHOLDER`，与
`config_load/persist.rs` 的 `sanitized_config_snapshot` 对持久化快照所做的一致。

这不是我发明的期望 —— 它是本仓库自己已经表达过的意图。现有测试
`persist_raw_config_encrypts_secret_store_resources_and_redacts_snapshots`
（`core/src/config_load/persist.rs:399`）逐条断言：`resources.spec_json` 加密、
`resource_versions.spec_json` 加密、`orchestrator_config_versions` 的
`config_yaml` 与 `config_json` **均不含明文且含占位符**。加密静态数据并脱敏
序列化快照，然后让一条导出命令原样吐出同一批值，只能是遗漏而非设计。

## 实际结果

明文。

```
PROBE_CONTAINS_CLEARTEXT=true
PROBE_CONTAINS_PLACEHOLDER=false
test result: ok. 1 passed; 0 failed; ... finished in 0.11s
```

## 复现步骤

两条独立推导，结论一致。

### 推导 1：执行（决定性）

把下面这段临时加进 `core/src/resource/export.rs` 末尾，然后
`cargo test -p agent-orchestrator --lib fr171_probe -- --nocapture`：

```rust
#[cfg(test)]
mod fr171_probe {
    use super::super::test_fixtures::make_config;
    use super::export_manifest_documents;

    #[test]
    fn probe_export_secret_value_visibility() {
        let mut config = make_config();
        config
            .ensure_project(Some(crate::config::DEFAULT_PROJECT_ID))
            .secret_stores
            .insert(
                "api-keys".to_string(),
                crate::config::SecretStoreConfig {
                    data: [("OPENAI_API_KEY".to_string(), "sk-probe-9d2f".to_string())].into(),
                },
            );
        let docs = export_manifest_documents(&config);
        let yaml = serde_yaml::to_string(&docs).expect("serialize");
        println!("PROBE_CONTAINS_CLEARTEXT={}", yaml.contains("sk-probe-9d2f"));
        println!(
            "PROBE_CONTAINS_PLACEHOLDER={}",
            yaml.contains(crate::secret_store_crypto::ENCRYPTED_PLACEHOLDER)
        );
    }
}
```

### 推导 2：代码链

1. **加载时解密到内存。** `core/src/persistence/repository/config.rs:293` 对每个
   资源（含 SecretStore）调用 `decrypt_resource_spec_json`。运行时必须如此 ——
   agent 的 `refValue` 注入需要真实值。上面那个既有测试的最后一段断言
   `loaded...data["OPENAI_API_KEY"] == "sk-secret-123"`，即内存明文是**契约**。
2. **脱敏只在持久化路径上。** `sanitized_config_snapshot`
   （`core/src/config_load/persist.rs:30-49`）只有两个调用方：
   `serialize_config_snapshot`（同文件 `:19`）与
   `core/src/persistence/repository/config.rs:92`。读取路径不经过它。
3. **导出读的是原始 config。** `service/resource/mod.rs:530-534` 的
   `export_manifests` 拿 `read_active_config(state)` 的 `config` 直接交给
   `export_manifest_documents`，中间无脱敏。
4. **导出逐字段克隆。** `core/src/resource/export.rs:132-138`：
   `spec: SecretStoreSpec { data: store.data.clone() }`。
5. **daemon 原样返回。** `crates/daemon/src/server/resource.rs:660-673` 的
   `manifest_export` 把 `content` 直接放进响应，无后处理。

## 影响

`orchestrator manifest export` 是文档中推荐的资源转储方式，也是四种不可查询
资源当前唯一的可见途径（见 FR-171）。操作员把它重定向到文件做备份或提 issue
时，会把全部 SecretStore 的明文值一并写出。

调用受 `authorize(server, &request, "ManifestExport")` 保护，所以这不是未认证
可达的路径；它是**纵深防御的缺口**：静态加密与快照脱敏所保护的东西，被一条
已授权的读命令绕过。

## 未核验

- **未经运行中的 daemon 端到端复现。** 推导 1 在纯函数层，推导 2 追到了处理器
  并确认无后处理，但没有真的 `apply` 一个 SecretStore 再 `orchestrator manifest
  export`。修复者应当先补这一条 —— 若端到端结果与此不符，说明本 ticket 漏读了
  一层。
- **`-o json` 与 `-o yaml` 只测了序列化后的字符串包含关系**，两种格式共用
  `builtin_docs`，故预期一致，但未分别断言。
- **未清点其他读取路径。** 本 ticket 只查了 `manifest export`。`ConfigDebug` RPC
  与 GUI 的资源目录是否有同类问题未查。FR-171 新增的 `get secretstore/<name>`
  已在渲染点脱敏（`service/resource/query.rs` 的 `describe_builtin_resource`），
  是本次唯一被主动关掉的一条。
- **未评估既有导出产物。** 若此前有人把 `manifest export` 的输出存档或提交过，
  那些文件里带着明文密钥；本 ticket 不含任何补救建议的可行性评估。

## 修复建议（非规定）

最小改动是让 `export_manifests` 走 `sanitized_config_snapshot` 而非原始 config。
但要先回答一个产品问题：**导出的清单还应当能被 `apply` 回去吗？** 若要，占位符
会把密钥写成字面量占位符再 apply 回去，那是数据损坏而不是脱敏 —— 所以脱敏的
同时可能需要让 apply 侧识别并拒绝占位符（`legacy_*` 那类可解析拒绝的形状）。
这个往返语义是本 ticket 没有裁决的部分。

回归测试应当断言**占位符在场**而不只是**明文缺席**：只断言缺席的话，一个把
SecretStore 整个从导出里漏掉的 bug 也会让测试变绿。
