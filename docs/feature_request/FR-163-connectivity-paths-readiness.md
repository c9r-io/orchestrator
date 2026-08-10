# FR-163: 连接、路径与就绪的单源化

## 优先级: P2

## 状态: Proposed

## 背景

所有计数 at `6678144d`，方法逐条注明；坐标由探索代理派生，治理时 step 0 重建。
本 FR 是 FR-160/ticket 系列（data-dir 启发式、fr013 接线、webhook 隔离）暴露的
同一根因的系统性收口：**同一事实多处派生**。

### 实测清单

- **数据目录/socket 解析共 6 套机制**（方法：rg 逐处核对）：
  `config_load::data_dir()`（规范：env→HOME，`core/src/config_load/mod.rs:37`）、
  `resolve_data_dir_from_db_path`（db 路径反推 + 布局启发式，
  `secret_store_crypto.rs:372`，e5977135 已加 env 压制但机制仍在）、
  `state.data_dir` 线程化（114 处 `.data_dir` 读取，`rg -c` 求和）、
  `discover_socket_path()`（client 独立实现，回退语义与规范版**不同**：
  `dirs::home_dir()` 为 None 时落到 cwd，`connect.rs:23-33`）、
  `lifecycle.rs:114` 第五套、`fs_watcher.rs:171` 直读 env 第六套。
- **control-plane 目录 5 处独立 `join("control-plane")`**：
  `control_plane.rs:346,383,704`、`protection.rs:269`、`uds_security.rs:187`。
- **db 路径两种布局同时活着**：flat（`bootstrap.rs:238`）与嵌套
  `data/agent_orchestrator.db`（`cli/commands/db.rs:151,168`、
  `control_plane.rs:992`），且被单测钉为特性（`secret_store_crypto.rs:743`）。
- **陈旧 socket 陷阱**：`connect.rs:61-66` 只探测 `socket.exists()`；daemon 在
  bind 时才清理陈旧 inode（`main.rs:990`）——崩溃后 CLI 锁死 UDS 分支、
  不再落到 TLS 配置，误报"daemon 没在跑"。DD-62 修的是镜像方向，此为残余。
- **`--bind` 静默关闭 UDS**：`main.rs:931-961` 的 else 分支互斥（FR-160 治理
  实测确认）；用户文档仅 `07-cli-reference.md:876` 一行 "default: Unix socket"，
  读作叠加而非互斥。`ORCHESTRATOR_SOCKET` 在 `docs/guide/` **零次出现**
  （rg 计数）。
- **就绪信号不存在**：`/health` 挂在 webhook 服务器、硬编码 "ok"、
  `--webhook-bind none` 时整个消失（`webhook.rs:51,248-250`、`main.rs:855`）；
  唯一真信号 `daemon_socket_ready` 事件只有已连上的客户端才读得到
  （`main.rs:1027`）。沉积物：QA 脚本 5 种就绪等待模式、23 个门禁手写同一
  `task list` 轮询且**5 种超时预算**（7.5s–25s，rg 循环边界统计）、113 个
  裸 sleep / 30 脚本。
- **quickstart 第 3 步 `orchestrator init` 实际冗余**：daemon 启动已同步跑完
  37 个迁移（`bootstrap.rs:239` 先于 socket bind ~700 行）。

## 需求

### 1. 路径解析单源

`config_load::data_dir()` 成为唯一派生点；client/daemon/security/fs_watcher
全部改为消费方（含 socket 路径 `data_dir()/orchestrator.sock` 的单一 helper）。
`resolve_data_dir_from_db_path` 的布局启发式在单源化后评估退役（能拿到真
data_dir 的调用方不该反推）；两种 db 布局二选一并写迁移说明，或具名保留
理由。control-plane 目录 join 收敛到一个 helper。

### 2. 陈旧 socket 的连接级探测

`exists()` 改为连接探测（或 connect 失败时继续落到下一发现分支）；错误信息
区分"socket 在但没人听"与"socket 不存在"。

### 3. 一个真正的就绪信号

产品级二选一（或都做）：gRPC 侧健康接口（含子系统状态：迁移、密钥、worker）
/ `orchestratord --wait-ready` 阻塞到可服务。`scripts/lib/gate_daemon.sh` 增加
`gate_daemon_wait_ready` 消费同一信号，收编 23 份手抄轮询与裸 sleep 主力。

### 4. 连接语义文档补全

`ORCHESTRATOR_SOCKET`、`--bind` 与 UDS 的互斥、发现顺序、陈旧 socket 的
自救——进用户指南（EN+ZH），不再只活在设计文档。quickstart 的 `init` 步骤
按需求 3 的结论修订（删除或改为 `--wait-ready` 示范）。

## 验收标准

- [ ] 路径派生机制数从 6 降至 1（+具名保留项若有），由 rg 派生的清单证明
- [ ] 陈旧 socket 场景行为测试：杀 daemon 留 inode → CLI 给出正确诊断或
      自动落到下一分支（负夹具：inode 在、无监听）
- [ ] 就绪信号存在且 QA 门禁至少 20 处轮询改为共享 helper（集合由 rg 派生，
      差集为空或具名）
- [ ] 文档含全部三个连接语义主题；guide 门禁（cli-doc-parity 族）通过
- [ ] 全量门禁与工作区测试绿

## 依赖与关联

- 承接 ticket `20260811-data-dir-heuristic-splits-key-paths`（已闭环，e5977135）
  与 DD-175 known limits；关联 DD-62（UDS 回退健壮性）。
- 与 FR-162 无代码耦合，可并行。

## 未核验项（明确标注）

- 6 套机制之外是否还有第 7 处（如 GUI/Tauri 侧）未清点——step 0 全仓重扫。
- `--wait-ready` 与现有 `daemon status` 的关系未设计（status 走 gRPC，
  未就绪时它自己也连不上——鸡蛋问题需在设计中解决）。
- 就绪信号覆盖哪些子系统（密钥播种是否入列）待 step 0 结合
  `bootstrap.rs:283`（诊断函数副作用播种，探索代理发现）一并裁决。
