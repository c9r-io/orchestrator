# FR-114 受控 Slack Sandbox 实测 Runbook

本 runbook 用于完成 FR-114 的非 CI live certification。它验证真实 Slack consent、同一官方 App 的多 workspace 隔离、badge → Skill task、离线补投、重授权、owner transfer、撤销、断开和备份恢复，同时避免把 workspace 私有信息或 credential 写入仓库、ticket、终端录屏和最终治理证据。

这不是生产上线指南。必须只使用专用 sandbox Slack workspace、合成消息和仓库内的 `echo` fixture。任何阶段出现跨 workspace 错投、credential 泄露、重复 task 或无法止损，都应立即停止，不要通过删除数据库行掩盖失败。

## 1. 完成定义

只有以下门禁全部通过，FR-114 的 live certification 才能标记为 PASS：

| Gate | 必须证明 |
|---|---|
| L0 | clean commit 上 FR-114 自动化 12 gates 全绿 |
| L1 | sandbox 官方 App manifest、TLS、Gateway 和两个隔离 daemon 前置条件正确 |
| L2 | GUI 一次真实 OAuth consent 创建一个 active connection 和默认 disabled Trigger |
| L3 | 取消、拒绝、过期和 callback replay fail closed，不产生幽灵 connection |
| L4 | 两个 sandbox workspace 使用同一 App，分别归属预期 daemon/project，不能跨租户读取或投递 |
| L5 | 两个 badge 选择不同 template/Skill/workflow，各创建一个 echo task；重复 reaction 不重复创建 task |
| L6 | daemon 离线时 Slack event 被 Gateway durable ack，重连后从 cursor 补投且只创建一个 task |
| L7 | reauthorize 保留 connection/Trigger identity，推进 generation/version，旧 generation 停止工作 |
| L8 | transfer 在旧 owner 与目标 owner 之间完成两阶段接力，任意时刻不超过一个有效 owner |
| L9 | 一个 workspace 的 uninstall/revocation 不影响另一个 workspace，并阻止被撤销 installation 的新 task |
| L10 | Gateway 与两个 daemon 的 SQLite 备份可恢复；disconnect 销毁 credential 但保留 source/route/task/audit 证据 |
| L11 | 最终证据只包含允许字段，隐私扫描通过，sandbox 被安全清理 |

若 L0–L11 任一失败，整次 certification 结论为 FAIL 或 BLOCKED，不能用自动化测试结果替代。

## 2. 人员、环境与测试拓扑

至少安排两种职责；同一人可以兼任，但最终证据应由另一位 reviewer 复核：

- **Slack sandbox admin**：能够安装/卸载 App、批准 OAuth、创建测试 channel 和 custom emoji。
- **Orchestrator operator**：能够部署 Gateway、启动两个隔离 daemon、执行 Admin RPC、备份与恢复。
- **Evidence reviewer**：只读取脱敏报告，确认没有 workspace、用户、channel/message URL 或 credential。

推荐拓扑：

```text
Sandbox workspace A ─┐
                     ├── one sandbox official Slack App ── public HTTPS Gateway
Sandbox workspace B ─┘                                      │
                                                            ├── daemon A / project A
                                                            └── daemon B / project B
                                                                 └── also preloads project A for transfer
```

硬性隔离要求：

- workspace A/B 都是非生产 workspace，不能包含真实客户、员工或项目讨论。
- Gateway 使用 sandbox 专属域名、Slack App、SQLite、master key 和 enrollment key。
- daemon A/B 使用不同 `ORCHESTRATORD_DATA_DIR`，不能复用开发或生产数据库。
- 两个 daemon 都只能访问本次 sandbox Gateway；Gateway/daemon SQLite 不能共享。
- 只运行 [`slack-managed-shared-oauth-fixture.yaml`](../../fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml) 中的确定性 `echo` agent，不运行真实 AI agent，不消耗 API credits。
- 测试 Slack 消息只写合成内容，例如 `FR114 sandbox echo request {run-id}`，不要包含代码、ticket、客户或人员信息。
- 两个 workspace 都准备两种 badge。为了零配置复跑，优先使用 Slack 内置 `:eyes:` 与 `:white_check_mark:`；若使用自定义 `:agent-implement:` / `:agent-docs:`，manifest 中必须使用不带冒号的名称。

## 3. 证据与 secret 处理规则

### 3.1 最终报告允许记录

- UTC/JST 日期、run ID、tester/reviewer；
- Git commit、二进制版本、App manifest SHA-256；
- installation/daemon 的匿名 SHA-256 digest；
- connection 的 `state`、`generation`、`version`、`last_acked_cursor`、`delivery_lag`、安全 `error_code`；
- canonical `request_id`；
- fixture template/workflow 名称、task 数量与终态；
- 每个 L0–L11 gate 的 PASS/FAIL/BLOCKED 与无敏感信息的说明。

### 3.2 最终报告禁止记录

- Slack workspace/team 名称或原始 team ID；
- Slack user、channel、message timestamp、message URL 或消息正文；
- Configuration Token、Client Secret、Signing Secret、bot token；
- OAuth authorize URL、code、state、poll secret 或 pairing secret；
- HTTP `Authorization`、raw Slack body、provider response body；
- Gateway/daemon SQLite 原始行、加密 ciphertext、完整环境变量或进程参数转储；
- task goal、Source 原始/normalized payload、受保护 permalink。

### 3.3 创建私密工作目录

不要在仓库内保存 live 原始输出。使用加密磁盘上的临时目录，并禁用 shell tracing：

```bash
set +x
umask 077
export RUN_ID="fr114-live-$(date -u +%Y%m%dT%H%M%SZ)"
export PRIVATE_RUN_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/${RUN_ID}.XXXXXX")"
export SAFE_REPORT="$PRIVATE_RUN_ROOT/certification-safe.md"
chmod 700 "$PRIVATE_RUN_ROOT"
```

若终端/CI 系统自动录屏或上传 stdout，不要在该终端执行 OAuth、`source connection connect/status/get` 或任何 SQLite 命令。不要启用 `set -x`、`RUST_LOG=trace`、HTTP body logging 或浏览器网络 HAR 录制。

## 4. 变量与双 Daemon 命令入口

在不包含 secret 的 operator shell 中设置：

```bash
export REPO_ROOT="$(git rev-parse --show-toplevel)"
export PROJECT_A="fr114-sandbox-a"
export PROJECT_B="fr114-sandbox-b"
export DAEMON_A_DATA="$PRIVATE_RUN_ROOT/daemon-a"
export DAEMON_B_DATA="$PRIVATE_RUN_ROOT/daemon-b"
export GATEWAY_DB="$PRIVATE_RUN_ROOT/gateway/gateway.db"
export GATEWAY_PUBLIC_URL="https://{sandbox-gateway-host}"
mkdir -p "$DAEMON_A_DATA" "$DAEMON_B_DATA" "$(dirname "$GATEWAY_DB")"
chmod 700 "$DAEMON_A_DATA" "$DAEMON_B_DATA" "$(dirname "$GATEWAY_DB")"
```

定义两个只切换 UDS 数据目录的 helper：

```bash
oa() { ORCHESTRATORD_DATA_DIR="$DAEMON_A_DATA" "$REPO_ROOT/target/release/orchestrator" "$@"; }
ob() { ORCHESTRATORD_DATA_DIR="$DAEMON_B_DATA" "$REPO_ROOT/target/release/orchestrator" "$@"; }
```

不要把 Gateway master/enrollment key 写入这个脚本。应从部署 secret backend 注入到 Gateway 和 daemon 进程。sandbox 中使用 `--uds-max-role admin` 仅限隔离单用户主机；共享机器应配置正式 UDS/TLS Admin policy。

## 5. L0：代码与自动化前置门禁

在加载任何 live secret 之前运行：

```bash
cd "$REPO_ROOT"
test -z "$(git status --porcelain)"
./scripts/qa/test-slack-managed-shared-oauth.sh
cargo build --release \
  -p orchestrator-slack-gateway \
  -p orchestratord \
  -p orchestrator-cli
cargo build --release -p orchestrator-gui --features custom-protocol
```

记录安全元数据：

```bash
export BUILD_COMMIT="$(git rev-parse HEAD)"
export MANIFEST_SHA256="$(shasum -a 256 deploy/slack/official-app-manifest.json | awk '{print $1}')"
target/release/orchestrator version
```

通过条件：工作树 clean；自动化显示 FR-114 12 gates 全绿，并包含 FR-113 aggregate；release binaries 构建成功。若失败，先修复代码，不能继续 live test。

## 6. L1：Gateway、TLS 与官方 App

### 6.1 部署前核对

- `GATEWAY_PUBLIC_URL` 是专用 HTTPS origin，无 query/fragment。
- `/slack/oauth/callback` 与 `/slack/events` 没有额外 path rewrite。
- TLS 证书链、hostname 和到期时间正确。
- 反向代理保留 exact raw body，不解压/重编码 Slack event body，不信任外部伪造的内部 header。
- 公网层已配置 body size、rate limit 和基础 abuse protection。
- `SLACK_GATEWAY_MASTER_KEY` 是 base64 32-byte key；`SLACK_GATEWAY_ENROLLMENT_KEY` 至少 32 bytes。
- sandbox/dev/staging/prod 之间不共享 App、DB 或 key。

```bash
curl -fsS "$GATEWAY_PUBLIC_URL/healthz"
curl -fsS "$GATEWAY_PUBLIC_URL/v1/capabilities" | jq '{protocol_version,supported_modes}'
openssl s_client -connect "{sandbox-gateway-host}:443" \
  -servername "{sandbox-gateway-host}" </dev/null 2>/dev/null \
  | openssl x509 -noout -subject -issuer -dates
```

### 6.2 Provision 或 validate App

Gateway 进程和以下命令必须从 secret backend 获得：

```text
SLACK_GATEWAY_PUBLIC_URL
SLACK_GATEWAY_DATABASE
SLACK_GATEWAY_MASTER_KEY
SLACK_GATEWAY_ENROLLMENT_KEY
```

首次创建专用 sandbox App 时，交互读取短期 Slack Configuration Token，经 stdin 传入：

```bash
read -rsp "Slack sandbox Configuration Token: " SLACK_CONFIGURATION_TOKEN
printf '%s' "$SLACK_CONFIGURATION_TOKEN" | \
  target/release/orchestrator-slack-gateway manifest provision \
    --manifest deploy/slack/official-app-manifest.json \
    --config-token-stdin
unset SLACK_CONFIGURATION_TOKEN
```

若 Gateway DB 已保存该 sandbox App 的加密 credentials，只运行 validate：

```bash
read -rsp "Slack sandbox Configuration Token: " SLACK_CONFIGURATION_TOKEN
printf '%s' "$SLACK_CONFIGURATION_TOKEN" | \
  target/release/orchestrator-slack-gateway manifest validate \
    --manifest deploy/slack/official-app-manifest.json \
    --config-token-stdin
unset SLACK_CONFIGURATION_TOKEN
```

立即撤销/丢弃短期 Configuration Token。确认 Slack App 配置为：

- bot scope 只有 `reactions:read`；
- events 为 `reaction_added`、`app_uninstalled`、`tokens_revoked`；
- Socket Mode、org deploy 和 token rotation 与当前 reviewed manifest 一致；
- callback/Request URL 精确指向 sandbox Gateway。

启动 Gateway（用现有 supervisor；以下 foreground 形式便于受控观察）：

```bash
RUST_LOG=info target/release/orchestrator-slack-gateway
```

日志只允许 stable IDs/digests、generation/version/cursor 和 error code。出现 secret、OAuth URL/state/code、workspace 名、raw body 或 message URL 立即执行第 16 节止损。

## 7. 启动两个隔离 Daemon 并加载 Echo Fixture

在两个独立 terminal 中启动 daemon；两边从 secret backend 获得相同 sandbox Gateway origin/enrollment key：

```bash
ORCHESTRATORD_DATA_DIR="$DAEMON_A_DATA" \
ORCHESTRATOR_SLACK_GATEWAY_URL="$GATEWAY_PUBLIC_URL" \
target/release/orchestratord --foreground --workers 2 --uds-max-role admin
```

```bash
ORCHESTRATORD_DATA_DIR="$DAEMON_B_DATA" \
ORCHESTRATOR_SLACK_GATEWAY_URL="$GATEWAY_PUBLIC_URL" \
target/release/orchestratord --foreground --workers 2 --uds-max-role admin
```

初始化并应用确定性资源：

```bash
oa init "$REPO_ROOT"
ob init "$REPO_ROOT"

oa apply --project "$PROJECT_A" \
  -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml
ob apply --project "$PROJECT_B" \
  -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml

# transfer 前置：daemon B 也必须拥有 project A 的 Workspace/Workflow/Template。
ob apply --project "$PROJECT_A" \
  -f fixtures/manifests/bundles/slack-managed-shared-oauth-fixture.yaml
```

检查两边 catalog：

```bash
oa source connection catalog -o json | jq '{protocol_version,gateway_configured,permalink_proxy,modes}'
ob source connection catalog -o json | jq '{protocol_version,gateway_configured,permalink_proxy,modes}'
```

期望 `managed_shared.available=true`、`gateway_configured=true`、`manual` 保持可用，`managed_dedicated` 明确不可用且不会降级成 shared。

## 8. L2/L3：真实 GUI OAuth 与失败路径

### 8.1 GUI 可发现性、刷新恢复和成功 consent

使用 daemon A 的环境启动 GUI：

```bash
ORCHESTRATORD_DATA_DIR="$DAEMON_A_DATA" target/release/orchestrator-gui
```

1. 从正常导航打开 **Sources → Connections**，不能使用隐藏 direct route。
2. 确认三种 provisioning 卡片同时可见；Dedicated 显示 unavailable，Existing app 仍可发现。
3. 选择 project A，在 **Instant — Official Orchestrator App** 点击 **Connect workspace**。
4. 在批准 Slack consent 前关闭并重新打开 GUI，确认 pending intent 自动恢复；浏览器 URL、OAuth state/token 不得出现在 DOM storage 检查结果中。
5. 在 Slack 页面确认是 workspace A 与 sandbox App，然后批准最小 scope。
6. 回到 Connections，等待同一个 intent 变成 completed。

将完整 CLI JSON 只保存到私密目录，并生成安全投影：

```bash
oa source connection list --project "$PROJECT_A" --provider slack -o json \
  >"$PRIVATE_RUN_ROOT/connection-a-private.json"
jq 'map({id,installation_id_digest,state,generation,version,trigger_name,last_acked_cursor,delivery_lag,last_error_code})' \
  "$PRIVATE_RUN_ROOT/connection-a-private.json"
```

期望恰好一个 `managed_shared/active` connection，一个自动 Trigger，且 Trigger 的 `reactionRouting` 初始为 `disabled`。保存私密 shell 变量，但不要写入最终报告：

```bash
export CONNECTION_A="$(jq -r '.[0].id' "$PRIVATE_RUN_ROOT/connection-a-private.json")"
export INSTALLATION_A="$(jq -r '.[0].installation_id' "$PRIVATE_RUN_ROOT/connection-a-private.json")"
export TRIGGER_A="$(jq -r '.[0].trigger_name' "$PRIVATE_RUN_ROOT/connection-a-private.json")"
export OWNER_A="$(jq -r '.[0].owner_daemon_id' "$PRIVATE_RUN_ROOT/connection-a-private.json")"
```

### 8.2 Cancel、deny、expiry、callback replay

按顺序使用新的 intent；每个子测都必须确认 connection 数量不增加：

1. **Cancel**：CLI `connect --no-open` 后，在未打开 authorize URL 前执行 `cancel`；再次查询期望 `cancelled`。
2. **Deny**：新建 intent，在 Slack consent 页面点击拒绝/取消；期望 intent terminal failure/cancel，且无新 connection。
3. **Expiry**：在独立窗口把 Gateway 以 `--intent-ttl-secs 60` 重启，创建但不完成 consent 的 intent，65 秒后查询；期望 `expired`/稳定 expiry error。随后以正常 600 秒 TTL 重启 Gateway。
4. **Callback replay**：成功 consent 后只在私密浏览器中刷新一次 callback 完成页；期望 replay fail closed，现有 connection ID/generation/Trigger 不改变。

命令形式：

```bash
oa source connection connect \
  --project "$PROJECT_A" \
  --label "FR114 cancelled intent" \
  --reason "live certification cancel path" \
  --idempotency-key "$RUN_ID-cancel" \
  --no-open

oa source connection cancel {intent_id} \
  --project "$PROJECT_A" \
  --reason "live certification cancel path" \
  --idempotency-key "$RUN_ID-cancel-confirm"

oa source connection status {intent_id} --project "$PROJECT_A" -o json \
  | jq '{status,error_code,expires_at}'
```

不要把 authorize URL、intent 原始 JSON、浏览器 history 或 callback URL写入证据。redirect/scope mismatch、tampered state 与 callback retry 的完整攻击矩阵由 L0 fake-provider tests 覆盖；live 环境只做上述不扩大权限的 provider checks。

## 9. L4：第二 Workspace 与跨租户隔离

在 daemon B 对 workspace B 重复 OAuth。可以使用 GUI，也可以使用 CLI 打开系统浏览器：

```bash
ob source connection connect \
  --project "$PROJECT_B" \
  --label "FR114 sandbox B" \
  --reason "live certification second tenant" \
  --idempotency-key "$RUN_ID-connect-b"
```

完成 consent 后：

```bash
ob source connection list --project "$PROJECT_B" --provider slack -o json \
  >"$PRIVATE_RUN_ROOT/connection-b-private.json"
export CONNECTION_B="$(jq -r '.[0].id' "$PRIVATE_RUN_ROOT/connection-b-private.json")"
export INSTALLATION_B="$(jq -r '.[0].installation_id' "$PRIVATE_RUN_ROOT/connection-b-private.json")"
export TRIGGER_B="$(jq -r '.[0].trigger_name' "$PRIVATE_RUN_ROOT/connection-b-private.json")"
export OWNER_B="$(jq -r '.[0].owner_daemon_id' "$PRIVATE_RUN_ROOT/connection-b-private.json")"
```

隔离断言：

- daemon A/project A list 只能看到 A；daemon B/project B list 只能看到 B。
- 用 A 的 connection ID 在 project B `get`，以及用 B 的 ID 在 project A `get`，都必须返回 NotFound/permission-safe error。
- workspace A 的 reaction 不能在 project B 产生 source/route/task；反向也一样。
- 两个 `installation_id_digest` 不同；每个 connection 只有一个 owner daemon/project。
- 不要把跨租户负测错误的完整 payload 保存到安全报告，只记录结果和 stable error code。

## 10. L5：配置两个 Badge 并创建不同 Echo Task

### 10.1 可复跑的 test-driver

首次人工认证可以直接在 Slack UI 发消息和添加 reaction。后续回归建议使用一个**独立、仅限 sandbox 的 test-driver App** 调用 `chat.postMessage` 和 `reactions.add`：

- 官方 Orchestrator App 继续只持有 `reactions:read`，不为测试扩大生产权限；
- test-driver App 只安装在受控 sandbox，持有 `chat:write` 与 `reactions:write`；
- 将 driver token 与 live identity 存在仓库外 mode `0600` 文件，绝不提交。

```bash
mkdir -p ~/.config/orchestrator/qa
cp config/qa/slack-live.env.example ~/.config/orchestrator/qa/fr114.env
chmod 600 ~/.config/orchestrator/qa/fr114.env
${EDITOR:-vi} ~/.config/orchestrator/qa/fr114.env

FR114_LIVE_ENV_FILE=~/.config/orchestrator/qa/fr114.env \
  ./scripts/qa/certify-slack-managed-live.sh
```

`certify-slack-managed-live.sh` 先运行确定性的 FR-114/FR-113 aggregate，再执行真实 Slack smoke。smoke 会验证 active/caught-up connection、无副作用 binding simulation、两个 badge → 两个不同 Skill task、reaction remove/re-add 幂等，以及最终 backlog/lease/Attention 归零；无论成功失败都会删除合成 Slack 消息和私密临时响应。OAuth、transfer、revocation、backup/restore 等低频生命周期仍按 L2-L10 人工步骤执行。

### 10.2 Binding 与真实 reaction

自动 Trigger 包含真实 installation/connection 引用。把它导出到私密临时文件：

```bash
oa get trigger "$TRIGGER_A" --project "$PROJECT_A" -o yaml \
  >"$PRIVATE_RUN_ROOT/live-routing-a.yaml"
chmod 600 "$PRIVATE_RUN_ROOT/live-routing-a.yaml"
```

用编辑器只做以下受控修改：

1. 保留自动生成的 `metadata.name`、`installationId` 和 `connectionRef`。
2. 在 `actorRoles` 中把本次 sandbox tester 的 Slack user ID 映射为 `operator`。
3. 先保持 `reactionRouting: disabled`。
4. 确认 action 使用 fixture 的 Workspace/Workflow。
5. 追加两个 `SourceTaskBinding`：

```yaml
---
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: fr114-live-implement
spec:
  triggerRef: "{trigger_a}"
  match:
    eventKind: reaction_added
    reaction: agent-implement
    targetKind: message
    channels: ["{private_channel_a}"]
  templateRef: managed-implement-from-slack
  allowedActorRoles: [operator]
  suspend: false
---
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: fr114-live-docs
spec:
  triggerRef: "{trigger_a}"
  match:
    eventKind: reaction_added
    reaction: agent-docs
    targetKind: message
    channels: ["{private_channel_a}"]
  templateRef: managed-docs-from-slack
  allowedActorRoles: [operator]
  suspend: false
```

先 dry-run、apply，再做正负 simulation：

```bash
oa apply --project "$PROJECT_A" --dry-run -f "$PRIVATE_RUN_ROOT/live-routing-a.yaml"
oa apply --project "$PROJECT_A" -f "$PRIVATE_RUN_ROOT/live-routing-a.yaml"

oa source binding simulate \
  --project "$PROJECT_A" \
  --installation "$INSTALLATION_A" \
  --reaction agent-implement \
  --channel "{private_channel_a}" \
  --actor "{private_operator_user_a}" -o json
```

再用错误 actor、错误 channel 和未知 badge 模拟，必须分别 fail closed。确认后把私密 Trigger manifest 的 `reactionRouting` 改为 `bindings` 并重新 apply。

在 workspace A 的两条不同合成消息上分别添加 `:agent-implement:` 和 `:agent-docs:`。只查看安全字段：

```bash
oa source automation status --project "$PROJECT_A" -o json
oa source automation list --project "$PROJECT_A" -o json \
  | jq 'map({binding_name,template_name,status,generation,attempt_count,error_code})'
oa task list --project "$PROJECT_A" -o json \
  | jq 'map({workflow,status})'
```

通过条件：

- 两个 route 分别固定到 `managed-implement-from-slack` 和 `managed-docs-from-slack`；
- 两个 task 分别使用 `slack-managed-implement` 和 `slack-managed-docs`，均由 echo agent 完成；
- 删除再重新添加相同 reaction，或等待 Slack retry 后，task 数量不增加；
- 不读取/记录 task goal，因为它包含受保护 permalink。

为 workspace B 配置一个同类的单 badge 私密 manifest并创建一个 echo task。确认 project A 的 task 计数不变，从而形成真实 cross-tenant canary。

## 11. L6：Daemon 离线积压与 Cursor 恢复

1. 记录 connection A 的安全基线：`version`、`last_acked_cursor`、`delivery_lag` 和 project A task 数量。
2. 正常停止 daemon A：`oa daemon stop`；不要停止 Gateway。
3. 在 workspace A 的新合成消息上添加一个已启用 badge。
4. 等待 Slack Events API 完成 provider retry window；Gateway 应持久化后返回成功，daemon 离线不应让 installation 失效。
5. 用同一 data dir、Gateway URL/enrollment key 重启 daemon A。
6. 轮询 connection/automation 状态，直到 `delivery_lag=0`、cursor 前进、route terminal，且 task 数量只增加 1。

```bash
oa source connection get "$CONNECTION_A" --project "$PROJECT_A" -o json \
  | jq '{state,generation,version,last_acked_cursor,delivery_lag,last_error_code}'
oa source automation status --project "$PROJECT_A" -o json
```

失败条件包括：Gateway 在 daemon 离线时丢失 event、重启从 cursor 0 重放、同一 badge 创建多个 task、lag 无界增长或产生静默 gap。

## 12. L7：Reauthorize

记录 A 当前 connection ID、Trigger、generation 和 version，然后通过 GUI 的 **Reauthorize** 或 CLI 发起：

```bash
export VERSION_A="$(oa source connection get "$CONNECTION_A" --project "$PROJECT_A" -o json | jq -r '.version')"
oa source connection reauthorize "$CONNECTION_A" \
  --project "$PROJECT_A" \
  --expected-version "$VERSION_A" \
  --reason "FR114 live credential rotation" \
  --idempotency-key "$RUN_ID-reauthorize-a"
```

完成 Slack consent 后验证：

- connection ID 与 Trigger 名不变；
- `generation` 至少加 1，`version` 单调增加，`reauthorized_at` 更新；
- stale `expected-version` 的第二次 reauthorize 被 CAS 拒绝；
- 新 badge 能创建一个 task，之前的 task/source/route/audit 仍存在；
- Gateway/daemon 日志不打印新旧 credential。

从 `source connection watch` 或 `audit list` 提取 canonical request ID，只记录 request ID 与安全 transition，不保存完整 audit payload：

```bash
oa audit list --project "$PROJECT_A" --target-id "$CONNECTION_A" -o json \
  | jq 'map({request_id,action,status,created_at})'
```

## 13. L8：两阶段 Owner Transfer

daemon B 已通过 workspace B connection 暴露稳定 owner ID；使用私密变量 `OWNER_B` 作为目标。先确认 daemon B 也已加载 project A fixture。

1. 记录 A 的当前 version/cursor/task count。
2. 正常停止 daemon B，模拟目标暂时不可达。
3. 在 daemon A 执行 transfer：

```bash
export VERSION_A="$(oa source connection get "$CONNECTION_A" --project "$PROJECT_A" -o json | jq -r '.version')"
oa source connection transfer "$CONNECTION_A" \
  --project "$PROJECT_A" \
  --expected-version "$VERSION_A" \
  --target-daemon-id "$OWNER_B" \
  --reason "FR114 controlled owner handoff" \
  --idempotency-key "$RUN_ID-transfer-a-to-b"
```

4. 旧 daemon A 应显示 `suspended`、`owner_transfer_pending_acceptance`，owner 已指向 B；旧 pairing 已被清除，不能再 claim/proxy/ack。
5. 使用 stale version 重复 transfer，必须 fail closed，不能产生第二个 owner/handoff。
6. 用同一 data dir 重启 daemon B；等待它 claim handoff、采用 connection/default Trigger/cursor 并 ack。
7. 在 daemon B/project A 查询同一 connection，期望 `active`，owner 为 B，cursor 不小于 transfer 前 cursor。
8. 将 `$PRIVATE_RUN_ROOT/live-routing-a.yaml` 在 daemon B/project A dry-run 后 apply，使 actor/channel binding 在新 owner 生效。
9. 在 workspace A 加一个新 badge；只能由 daemon B/project A 创建一个 task，daemon A task count 不变。

任何时刻观察到两个 active owner、旧 daemon 仍能处理 permalink/delivery、cursor 回退或目标需要复制 pairing 才能恢复，都判定 FAIL。

## 14. L9：Workspace 级 Revocation 隔离

此时 workspace A connection 已由 daemon B/project A 持有，workspace B connection 仍由 daemon B/project B 持有。

1. 从 Slack workspace B 管理界面卸载 sandbox App；不要删除整个官方 App。
2. 等待 `app_uninstalled`/token revocation 到达 Gateway 和 daemon B。
3. project B connection 应变为 `revoked` 或带稳定 revocation error 的 attention state，并停止新 delivery/proxy/task。
4. 对 workspace B 新消息加 badge，不得创建 task。
5. workspace A connection 必须继续 active；在 A 添加 badge仍只创建一个 project A task。
6. project A/B 的既有 source、route、task、Attention 与 audit 继续可查。
7. 对 workspace B 重新执行 OAuth，验证同一逻辑 identity 安全恢复或按产品返回明确的新 consent 结果；不得影响 A 的 owner/generation/cursor。

如果卸载 B 导致 A revoked、A event进入 project B、或 B 在 revoked 后仍能创建 task，立即执行止损并判定 FAIL。

## 15. L10：Backup/Restore 与 Disconnect

### 15.1 写入止损并排空

在备份前 suspend 两个真实 Trigger，启用 daemon maintenance，等待 automation backlog/active lease 为 0：

```bash
ob trigger suspend "$TRIGGER_A" --project "$PROJECT_A"
ob trigger suspend "$TRIGGER_B" --project "$PROJECT_B"
oa daemon maintenance --enable
ob daemon maintenance --enable
ob source automation status --project "$PROJECT_A" -o json
ob source automation status --project "$PROJECT_B" -o json
```

确认 sandbox 中不再添加 reaction，然后依次停止 daemon A、daemon B 和 Gateway。保留同一组 master/enrollment key；缺少 encryption key 的 SQLite 备份不算可恢复备份。

### 15.2 创建并校验独立备份

```bash
sqlite3 "$GATEWAY_DB" "PRAGMA integrity_check;"
sqlite3 "$DAEMON_A_DATA/agent_orchestrator.db" "PRAGMA integrity_check;"
sqlite3 "$DAEMON_B_DATA/agent_orchestrator.db" "PRAGMA integrity_check;"

sqlite3 "$GATEWAY_DB" ".backup '$PRIVATE_RUN_ROOT/gateway-backup.db'"
sqlite3 "$DAEMON_A_DATA/agent_orchestrator.db" ".backup '$PRIVATE_RUN_ROOT/daemon-a-backup.db'"
sqlite3 "$DAEMON_B_DATA/agent_orchestrator.db" ".backup '$PRIVATE_RUN_ROOT/daemon-b-backup.db'"

sqlite3 "$PRIVATE_RUN_ROOT/gateway-backup.db" "PRAGMA integrity_check;"
sqlite3 "$PRIVATE_RUN_ROOT/daemon-a-backup.db" "PRAGMA integrity_check;"
sqlite3 "$PRIVATE_RUN_ROOT/daemon-b-backup.db" "PRAGMA integrity_check;"
```

只记录 `integrity_check=ok` 与 schema version，不导出表内容：

```bash
sqlite3 "$PRIVATE_RUN_ROOT/gateway-backup.db" \
  "SELECT MAX(version) FROM gateway_schema_migrations;"
sqlite3 "$PRIVATE_RUN_ROOT/daemon-b-backup.db" \
  "SELECT MAX(version) FROM schema_migrations;"
```

### 15.3 原位恢复演练

先把当前数据库保留为私密 rollback copy，再从 backup 建立新文件：

```bash
mv "$GATEWAY_DB" "$GATEWAY_DB.pre-restore"
mv "$DAEMON_A_DATA/agent_orchestrator.db" "$DAEMON_A_DATA/agent_orchestrator.db.pre-restore"
mv "$DAEMON_B_DATA/agent_orchestrator.db" "$DAEMON_B_DATA/agent_orchestrator.db.pre-restore"

sqlite3 "$PRIVATE_RUN_ROOT/gateway-backup.db" ".backup '$GATEWAY_DB'"
sqlite3 "$PRIVATE_RUN_ROOT/daemon-a-backup.db" ".backup '$DAEMON_A_DATA/agent_orchestrator.db'"
sqlite3 "$PRIVATE_RUN_ROOT/daemon-b-backup.db" ".backup '$DAEMON_B_DATA/agent_orchestrator.db'"
chmod 600 "$GATEWAY_DB" "$DAEMON_A_DATA/agent_orchestrator.db" "$DAEMON_B_DATA/agent_orchestrator.db"
```

以原 key 和原配置启动 Gateway，再启动 daemon A/B。验证：

- Gateway `/healthz` 与 `/v1/capabilities` 正常；
- `oa db status`/`ob db status` 没有 pending/failed migration；
- connection state、owner、generation/version/cursor与备份时一致；
- transfer 后的 owner 仍是 daemon B；
- task/source/route/audit 计数和安全终态保留；
- 未出现重复 task、credential decrypt error 或跨租户记录。

恢复确认后才删除 `.pre-restore` rollback copy。

### 15.4 Disconnect 与证据保留

在当前 active owner daemon B 上获取最新 version，执行 reviewed disconnect：

```bash
export VERSION_A="$(ob source connection get "$CONNECTION_A" --project "$PROJECT_A" -o json | jq -r '.version')"
ob source connection disconnect "$CONNECTION_A" \
  --project "$PROJECT_A" \
  --expected-version "$VERSION_A" \
  --reason "FR114 live certification cleanup" \
  --idempotency-key "$RUN_ID-disconnect-a"
```

期望 connection 为 `disconnected`，新 delivery/proxy停止；使用 `--include-disconnected` 仍能读取安全 connection evidence，已有 task/source/route/audit 数量不减少。对 workspace B 重复 cleanup，或从 Slack 管理界面卸载 installation。

## 16. 立即止损条件

发生下列任一情况，立即停止新 reaction/OAuth，suspend Trigger，停止 daemon 与 Gateway，并由 Slack admin 撤销受影响 installation：

- A 的 event/task 出现在 B，或 B 出现在 A；
- OAuth/code/state/token/signing secret/pairing/private URL 出现在日志、GUI storage 或证据；
- transfer 后旧 owner 仍能 claim/proxy/ack；
- revoked/disconnected installation 仍创建 task；
- 同一 badge identity 创建多个 canonical task；
- Gateway ack 但 durable queue/source evidence缺失；
- DB restore 需要绕过 schema/key 校验或删除 migration row；
- 无法确定当前唯一 owner 或当前有效 credential generation。

止损时不要：

- 删除 source/route/task/audit 行；
- 手工复制 pairing、bot token 或 SQLite credential row；
- 重复 OAuth 抢占 owner；
- 把原始日志/数据库上传到公开 issue；
- 在未确认影响范围前轮换共享 App 凭证，导致两个 workspace 同时失效。

保留加密私密副本并建立安全事件记录；治理报告只写匿名 digest、时间、stable error code、影响 gate 和处置状态。

## 17. L11：最终隐私扫描与报告

对 Gateway/daemon 日志、GUI local/session storage 截图、命令输出和拟提交报告执行人工+自动扫描。至少搜索：

```text
xoxb-  xoxp-  xoxa-  xoxr-
oauth/v2/authorize  code=  state=
Authorization:  signing  client_secret  pairing
slack.com/archives/  hooks.slack.com
workspace/team/user/channel 的本次 private canary
```

不要对 SQLite、浏览器 profile 或 credential store做字符串 dump 后把结果保存到报告；扫描应在私密边界内执行，只记录“0 forbidden matches”或 stable incident ID。

安全报告模板：

```markdown
# FR-114 Controlled Slack Sandbox Certification

- Run ID: {run_id}
- Date/time: {utc_and_jst}
- Build commit: {git_sha}
- Orchestrator version: {version}
- Official App manifest SHA-256: {digest}
- Installation digests: {digest_a}, {digest_b}
- Daemon digests: {digest_a}, {digest_b}
- Tester: {name_or_role}
- Reviewer: {name_or_role}

| Gate | Result | Safe evidence |
|---|---|---|
| L0 Automated aggregate | PASS/FAIL | 12 FR-114 gates; duration |
| L1 Gateway/App/TLS | PASS/FAIL | manifest digest; health/capability |
| L2 GUI OAuth | PASS/FAIL | active; generation/version; request ID |
| L3 OAuth failures | PASS/FAIL | terminal states and stable error codes |
| L4 Tenant isolation | PASS/FAIL | distinct digests; negative read/delivery result |
| L5 Two badges | PASS/FAIL | two fixture workflows; task counts/status |
| L6 Offline recovery | PASS/FAIL | cursor before/after; lag=0; task delta=1 |
| L7 Reauthorize | PASS/FAIL | same connection; generation/version transition |
| L8 Transfer | PASS/FAIL | owner digest transition; cursor monotonic; request ID |
| L9 Revocation | PASS/FAIL | B revoked; A active; no new B task |
| L10 Restore/disconnect | PASS/FAIL | integrity/schema; evidence retained |
| L11 Privacy/cleanup | PASS/FAIL | 0 forbidden matches; installations removed |

Overall: PASS/FAIL/BLOCKED
Open observations: {safe_error_codes_or_none}
```

用本次随机 salt 对 raw daemon ID 做匿名 digest；salt 留在私密目录，不提交：

```bash
export RUN_SALT="$(openssl rand -hex 32)"
printf '%s:%s' "$RUN_SALT" "$OWNER_A" | shasum -a 256
printf '%s:%s' "$RUN_SALT" "$OWNER_B" | shasum -a 256
```

SourceConnection 已提供 `installation_id_digest`，直接记录它即可。报告复核通过后，只把上述安全表格追加到 FR-114 `Implementation Evidence` 和 QA-162 checklist；不要提交 `$PRIVATE_RUN_ROOT`、browser profile、SQLite backup 或 live routing YAML。

## 18. 清理与 FR 关闭

1. 确认两个 sandbox installation 已 disconnect/uninstall，所有真实 Trigger 已 suspend。
2. 停止两个 daemon 与 Gateway。
3. 撤销短期 Slack Configuration Token；按 sandbox policy 轮换/销毁 enrollment/master secret。
4. 删除私密 routing YAML、browser profile、SQLite backup 和 daemon data dir；只有已审核的安全报告可进入治理文档。
5. 再运行一次 `git status --short`，确认 private artifacts 未进入仓库。
6. Evidence reviewer 复核 L0–L11、隐私扫描和 cleanup。
7. 仅当全部 PASS 时，将 QA-162 Scenario 1–5 更新为 PASS、补充 run ID/日期，并把 FR-114 剩余 live acceptance criteria勾选后进入 closure review。

若 provider outage、Slack admin 权限或基础设施窗口导致无法执行，可标记 BLOCKED 并记录安全原因；不能把未执行步骤写成 PASS。

## 相关文档

- [Managed Slack Connection 用户指南](slack-managed-connections.md)
- [Slack Reaction → Skill 自动化指南](slack-reaction-skill-automation.md)
- [FR-114 QA](../qa/orchestrator/162-managed-slack-connection-shared-oauth.md)
- [FR-114 Design](../design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md)
- [Slack Gateway Threat Model](../security/slack-gateway-threat-model.md)
