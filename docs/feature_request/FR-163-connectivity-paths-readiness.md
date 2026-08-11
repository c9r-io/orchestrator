# FR-163: 连接、路径与就绪的单源化

## 优先级: P2

## 状态: In Progress（需求 1、2 已闭环，见 DD-178/QA-215；需求 3、4 待第二轮）

## 背景

本 FR 是 FR-160/ticket 系列（data-dir 启发式、fr013 接线、webhook 隔离）暴露的
同一根因的系统性收口：**同一事实多处派生**。

原始清单计于 `6678144d`。**治理 step 0 已在 `70c85cba` 全量重建**（22 个提交之后），
逐条结果见下；被推翻的四条已就地改写，坐标已重钉。凡未标注 `(step 0 已核验)`
的数字均为原稿遗留，不得据以规划。

### 实测清单（`70c85cba`，方法逐条注明）

- **数据目录派生共 4 处独立机制**（方法：`rg 'ORCHESTRATORD_DATA_DIR|home_dir\(\)'
  --type rust`，逐处读源；step 0 已核验）：
  `config_load::data_dir()`（规范：env→home→cwd 相对 `.orchestratord`，
  `core/src/config_load/mod.rs:37`）、
  `discover_socket_path()`（client 独立实现，
  `crates/orchestrator-client/src/connect.rs:23`）、
  `resolve_data_dir_from_db_path`（db 路径反推 + `data` 目录名启发式，
  `crates/orchestrator-security/src/secret_store_crypto.rs:372`，e5977135 已加 env
  压制但机制仍在；生产调用方 3 处，
  `core/src/persistence/repository/config.rs:252,467,497`）、
  `crates/daemon/src/fs_watcher.rs:171`（裸 env 读，**无 home 回退**）。

  原稿另计入的两项**不是派生机制**（step 0 更正，属类别混淆）：
  `lifecycle::socket_path(data_dir)`（`crates/daemon/src/lifecycle.rs:114`）是接受
  data_dir 为入参的纯 helper，自身不派生任何路径；`state.data_dir` 线程化
  （115 处 `.data_dir` / 34 文件，`rg -o` 计；原稿 114 为 `rg -c` 的行数和）是
  **消费方模式**，正是需求 1 想要的终局形态，不是缺陷。

- **socket 文件名在 2 处独立拼写**（step 0 已核验）：`lifecycle.rs:115`
  的 `data_dir.join("orchestrator.sock")` 与 `connect.rs:28` 的
  `PathBuf::from(dir).join("orchestrator.sock")`。这是 client 与 daemon 之间真实的
  重复；原稿描述的"回退语义与规范版**不同**"经核验**不成立**——
  `home_dir()` 为 None 时 `config_load` 落到 `.orchestratord`、`connect` 落到
  `./.orchestratord/orchestrator.sock`，二者语义相同（open ticket
  `20260811-stale-socket-traps-cli-discovery.md` 的"bonus defect"沿用了同一误判；
  该 ticket 已随需求 2 闭环删除，订正内容并入 DD-178）。

- **`fs_watcher` 的缺陷方向与原稿相反**（step 0 更正）：`fs_watcher.rs:171` 只在
  `ORCHESTRATORD_DATA_DIR` **已设置**时跳过 daemon 数据目录。默认部署（env 未设、
  data_dir 走 home 回退）下该跳过是空操作——一个 root 落在 `$HOME` 的
  filesystem trigger 会监视 daemon 自己的 `~/.orchestratord`。这是可断言的行为缺陷，
  不是命名不一致。

- **control-plane 目录 5 处生产 `join("control-plane")`**（step 0 已核验，坐标不变）：
  `control_plane.rs:346,383,704`、`protection.rs:269`、`uds_security.rs:187`。
  全仓 8 处，另 3 处 `uds_security.rs:291,305,318` 在 `#[cfg(test)]` 内。

- **db 布局只有一种活在生产**（step 0 推翻原稿）：原稿称 flat 与嵌套
  `data/agent_orchestrator.db` 两种布局同时活着。全部 7 处嵌套写法均在
  `#[cfg(test)]` 模块内（`secret_store_crypto.rs:680,745,779` 之 672；
  `secret_key_lifecycle.rs:833` 之 827；`cli/commands/db.rs:151,168` 之 144；
  `control_plane.rs:992` 之 846）。生产只写 flat（`bootstrap.rs:238`）。
  所谓"嵌套布局"并非代码会写出的布局，而是**调用方把自己的 data_dir 命名为
  `data`** 时启发式的误判——正是 e5977135 的 env 压制所修的那个 QA gate 场景。
  **后果：不需要任何 db 迁移**；需求 1 中"两种布局二选一并写迁移说明"缩减为
  "退役 `data` 目录名启发式分支及其单测钉子（`secret_store_crypto.rs:743`）"。
  非 Rust 残留一处值得具名：`scripts/run-full-qa.sh:11` 导出
  `ORCHESTRATOR_SOCKET=data/orchestrator.sock`（相对路径 + 嵌套 `data/` 假设）。

- **陈旧 socket 陷阱（范围已收窄）**：`connect.rs:63-66` 只探测 `socket.exists()`；
  daemon 在 bind 时清理陈旧 inode（`main.rs:991`）**且在正常关停时也清理**
  （`lifecycle::cleanup`，`main.rs:1170`）。故陷阱仅在**崩溃/SIGKILL** 后成立，
  而非"任何一次停机后"。命中时 CLI 锁死 UDS 分支、重试 3 次后报错，
  不再落到 TLS 配置发现（`connect_uds`，`connect.rs:77-133`）。
  DD-62 修的是镜像方向，此为残余。与 ticket
  `20260811-stale-socket-traps-cli-discovery.md` 同一标的；**已随第一轮闭环，
  ticket 已删除**。

- **`--bind` 静默关闭 UDS**：`main.rs:932-961` 的 `if let Some(addr) = args.bind`
  / `else { …UDS… }` 互斥，无任何警告行（step 0 已核验）；用户文档仅
  `docs/guide/07-cli-reference.md:903`（ZH `docs/guide/zh/07-cli-reference.md:898`）
  一行 "TCP bind address (default: Unix socket)"，读作叠加而非互斥。
  `ORCHESTRATOR_SOCKET` 在 `docs/guide/` **零次出现**（`rg -c`，step 0 已核验）。

- **就绪信号不存在**：`/health` 挂在 webhook 服务器、硬编码 `"ok"`、
  `--webhook-bind none` 时整个消失（`webhook.rs:62,309`、`main.rs:856`）；
  唯一真信号 `daemon_socket_ready` 事件只有已连上的客户端才读得到
  （`main.rs:1028`）。沉积物（step 0 重新派生，方法：扫描
  `for _ in {1..N}; do` 循环体中丢弃输出的 `task list` 存活探针）：
  **24 处轮询 / 23 个门禁**手写同一探针，**5 种超时预算**——
  7.5s×1、10s×4、15s×6、20s×12、25s×1（原稿"23 个门禁 / 5 种预算 / 7.5–25s"
  完全命中）。裸 `sleep` 全仓 **123 处 / 36 文件**（原稿 113/30 为 `6678144d` 值）。
  `scripts/lib/gate_daemon.sh` 现有 `gate_daemon_pid_from_file` / `gate_daemon_alive`
  / `gate_daemon_stop`，**无任何 wait/ready helper**。

- **quickstart 第 3 步 `orchestrator init` 冗余程度强于原稿描述**（step 0 更正）：
  `init` 是一次 gRPC 调用（`crates/cli/src/commands/mod.rs:107-113` → `InitRequest`），
  **没有活着的 daemon 就根本跑不起来**，而活着的 daemon 早已在 bind socket 前
  同步跑完 `initialize_runtime`（`bootstrap.rs:236-249`，`main.rs:272` 与
  bind 处 `main.rs:992` 相隔 720 行）。因此
  `docs/guide/01-quickstart.md:39-43`（及 ZH 同位）"This creates the SQLite schema"
  一句**作为陈述即为假**。迁移总数为 **38**（`migration_chain_tests.rs:1256` 钉死；
  原稿 37 为 `6678144d` 值）。

## 需求

### 1. 路径解析单源

> **已交付（第一轮）**。落点与本节的设想有一处不同并已记入 DD-178：单源模块是
> **`orchestrator_config::paths`**，不是 `core::config_load` 或 `lifecycle` 的提升版
> ——`orchestrator-client` 不依赖 `core`，helper 必须放在两边都够得着的地方；
> `core::config_load::data_dir` 改为 re-export（与同文件 `now_ts` 同构）。

`config_load::data_dir()` 成为唯一 data_dir 派生点；socket 路径收敛到单一
helper（`lifecycle::socket_path` 或其提升版），由 client 与 daemon 共同消费，
消除 `orchestrator.sock` 的第二处拼写。`fs_watcher.rs:171` 改为消费
`data_dir()`，使默认部署下的自我监视跳过真正生效。
`resolve_data_dir_from_db_path` 的 `data` 目录名启发式退役（能拿到真 data_dir
的调用方不该反推），连同 `secret_store_crypto.rs:743` 的单测钉子；
**无需 db 迁移**（见背景更正）。control-plane 目录 join 收敛到一个 helper。

`discover_socket_path()` 保留为独立函数——它消费 `ORCHESTRATOR_SOCKET`，
一个 `data_dir()` 不认识的变量——但其 data_dir 分支改为调用 `data_dir()`。
**终局目标是 4 → 2（规范 `data_dir()` + 一个消费它的 socket 发现函数），
不是 4 → 1**；`.data_dir` 线程化保持不变，那是目标形态。

### 2. 陈旧 socket 的连接级探测

> **已交付（第一轮）**，见 DD-178 与 QA-215 S4。ticket
> `20260811-stale-socket-traps-cli-discovery.md` 已删除。

`exists()` 改为连接探测（或 connect 失败时继续落到下一发现分支）；错误信息
区分"socket 在但没人听"与"socket 不存在"。

### 3. 一个真正的就绪信号

产品级二选一（或都做）：gRPC 侧健康接口（含子系统状态：迁移、密钥、worker）
/ `orchestratord --wait-ready` 阻塞到可服务。`scripts/lib/gate_daemon.sh` 增加
`gate_daemon_wait_ready` 消费同一信号，收编 24 处手抄轮询与裸 sleep 主力。

设计起点已由 step 0 澄清：`daemon status` **不走 gRPC**——它读
`data_dir()/daemon.pid` 并用 `kill -0` 探活（`crates/cli/src/commands/daemon.rs:14-20,
45-57`）。原稿列为未核验项的"鸡蛋问题"**不存在**：连接无关的存活面已经就位，
缺的是它上面的**就绪**维度。

### 4. 连接语义文档补全

`ORCHESTRATOR_SOCKET`、`--bind` 与 UDS 的互斥、发现顺序、陈旧 socket 的
自救——进用户指南（EN+ZH），不再只活在设计文档。quickstart 的 `init` 步骤
按需求 3 的结论修订（删除或改为 `--wait-ready` 示范），并订正
"This creates the SQLite schema" 这句假陈述（EN+ZH）。

## 验收标准

- [x] data_dir 派生机制从 4 降至 1、socket 路径拼写从 2 降至 1。实际交付强于此条：
      `connectivity-path-single-source.rb` 断言 **7 个布局名字各只被拼写一次**
      （data dir / socket / pidfile / db / control-plane dir / client dir / env var），
      作用范围由 git 派生并带镜像条件。具名保留项两处：
      `discover_socket_path` 的 `ORCHESTRATOR_SOCKET` 分支，
      以及 `runner/policy.rs` 中筛查 `kill $(cat .../daemon.pid)` 的子串守卫。
- [x] `fs_watcher` 行为断言（`crates/daemon/src/fs_watcher.rs` 单测）。负夹具双向实测：
      改回读 env → 红；`Path::starts_with` 降级为字符串前缀 → 另一条红。
- [x] 陈旧 socket 场景行为测试（`scripts/qa/test-stale-socket-discovery.sh`，7 项）。
      负夹具实测 `4 passed, 3 failed`。断言诊断文本与 RPC 结果，从不断言退出码。
- [ ] 就绪信号存在且 QA 门禁至少 20 处轮询改为共享 helper —— **第二轮**
- [ ] 文档含全部三个连接语义主题；quickstart 假陈述已订正（EN+ZH）—— **第二轮**
- [x] 全量门禁与工作区测试绿（第一轮范围）

### 第一轮额外交付（不在原验收标准内）

- [x] 修复自动发现第 4 步：daemon 写 `~/.orchestrator/control-plane/config.yaml`，
      客户端却找 `~/.orchestratord/...`，差一个字符，该分支**从未生效过**。
      由陈旧 socket 门禁的场景 A 发现。10 个以上 QA 门禁手工设
      `ORCHESTRATOR_CONTROL_PLANE_CONFIG` 绕行即是其沉积物。
- [x] `scripts/lib/gate_daemon.sh` 新增 `gate_daemon_kill_hard`：受控的崩溃停机，
      供以"不干净退出留下什么"为主语的门禁使用。

## 依赖与关联

- 承接 ticket `20260811-data-dir-heuristic-splits-key-paths`（已闭环，e5977135）；
  关联 DD-62（UDS 回退健壮性）；已覆盖并删除 ticket
  `20260811-stale-socket-traps-cli-discovery.md`（第一轮）。
- **原稿的 "DD-175 known limits" 引用不成立**（step 0 更正）：DD-175 是
  `175-provider-isolation-login-shell.md`，全文无 `data_dir` 或该启发式的任何提及。
  全仓没有任何 DD 记录这条 known limit——唯一记录它的文档就是本 FR。
  这本身是 skill §6.4 的实例：知识只停在 FR 里，闭环即蒸发。
- 与 FR-162 无代码耦合，可并行。

## 未核验项（明确标注）

- （已消解）第 7 处派生机制：`gui/` 下无 `src-tauri`、无任何 Rust 源；
  GUI 侧对 `ORCHESTRATOR_SOCKET` 的唯一提及是 `gui/src/lib/i18n.ts:184-185`
  的帮助文案。step 0 已核验，无第 5 处机制。
- （已消解）`--wait-ready` 与 `daemon status` 的鸡蛋问题——见需求 3。
- 就绪信号覆盖哪些子系统（密钥播种是否入列）仍待裁决。已核验的相关事实：
  诊断函数 `run_key_lifecycle_diagnostics`（`bootstrap.rs:281`）经
  `load_keyring` → `load_keyring_legacy` → `ensure_secret_key` 确有播种副作用，
  但同一路径上 `initialize_runtime` 已在 `bootstrap.rs:241` 直接播种过，
  故该副作用在生产时序下被抢先，属**可达但不生效**。
