# FR-089: SecretStore 加密密钥紧急恢复机制

| 字段 | 值 |
|------|---|
| **优先级** | P2 |
| **状态** | Proposed |
| **关联** | QA-64 S4, DD-027 (secretstore-key-lifecycle) |

## 背景

SecretStore 密钥生命周期支持 rotate 和 revoke 操作。当所有密钥均进入终态（retired/revoked）后，系统无法加密新 SecretStore 资源，也无法解密已有资源。Daemon 启动时如需加载 SecretStore 会直接失败。

## 问题描述

当前 CLI 没有从"全部密钥终态"恢复的路径：

- `orchestrator secret key rotate` 需要一个 active key 作为基础，当无 active key 时报错 "no active key found; cannot begin rotation"
- `orchestrator apply` 对 SecretStore 资源报错 "no active encryption key"
- 无 `secret key bootstrap` 或 `secret key activate` 命令
- 唯一恢复方式是直接修改数据库（将某个 key 的 state 从 retired/revoked 改为 active）

复现步骤（来自 QA-64 S4）：

1. `orchestrator secret key rotate` — 创建新 key，旧 key 变为 retired
2. `orchestrator secret key revoke <new_key_id>` — 新 key 变为 revoked
3. 此时所有 key 均为终态，系统不可用

## 验收标准

1. 提供 CLI 命令从无 active key 状态恢复（如 `secret key bootstrap` 或 `secret key activate <key_id>`）
2. 恢复后 daemon 可正常启动并加载 SecretStore 资源
3. 恢复操作记录审计事件（`key_activated` 或 `key_bootstrapped`）
4. `secret key revoke` 增加安全提示：当被撤销的是最后一个 active key 时，警告用户并要求 `--force` 确认

## 来源

- QA ticket: `docs/ticket/qa-64-key-revocation-no-recovery-path_20260331.md`
- 全量 QA 回归中 daemon 启动失败，需手动修改 DB 中 `secret_keys.state` 才能恢复
