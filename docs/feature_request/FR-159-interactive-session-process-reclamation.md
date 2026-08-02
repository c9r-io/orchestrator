# FR-159: 交互会话进程回收 — 孤儿泄漏与 OS 层回收缺口

## 优先级: P1

## 状态: Proposed

## 背景

2026-08-02 在开发机上的实测：**28 个 `session-control-mock` 会话进程存活最长 19 天，其中 23 个已被 reparent 到 `init`（`ppid=1`）；另有 6 个 `./target/debug/orchestratord` 同样 `ppid=1`，端口 19394–19399 仍在 LISTEN**。这些不是僵尸进程——全系统 `Z` 状态计数为 0——而是活着的孤儿，且在持续消耗 CPU：当前合计 28.1%（约 0.28 核），累计已烧掉 **133.7 小时 CPU 时间**。

泄漏路径由四层缺口叠加而成，每一层单独看都是合理设计：

1. **子进程自成进程组，因而免疫于守护进程之死**。`crates/orchestrator-runner/src/runner/spawn.rs:69` 用 `process_group(0)` 让每个子进程成为组长，这是 `kill_child_process_group`（同文件 `:187`，发 `kill(-pid, SIGKILL)`）能够连坐整棵子树的前提。代价是：daemon 被 SIGKILL 或崩溃时，信号不会波及这些独立进程组。

2. **会话子进程从未被登记进 `runtime.child`，因而所有既有 kill 路径都够不到它**（2026-08-03 治理核实，修正本条最初的表述）。原稿写作"`shutdown_running_tasks` 只遍历 `state.running`，而持久化的 interactive session 不在该集合内"——机制不是这样。`crates/orchestrator-scheduler/src/scheduler/phase_runner/spawn.rs:234` 对 tty 会话走 `tty_early_return` 分支并在 `:252` 提前返回，于是 `:315` 的 `*child_lock = Some(child)` 对会话**永远不可达**。

   调度器里每一条回收路径都经由 `runtime.child`：`shutdown_running_tasks`（`runtime.rs:163`）、`stop_task_runtime_for_delete`（`runtime.rs:150`）、步骤超时（`wait.rs:73`）、stall 自动 kill（`wait.rs:182`）、跨进程 pause（`wait.rs:207`）。**没有一条能触及任何 interactive session**——无论其任务是否在途，也无论关停是否优雅。

   唯一还能碰到会话的是 `spawn.rs:60` 的 `kill_on_drop`，而它是**单 PID 而非进程组**（tokio 的 `Child::kill` 发 `kill(pid, SIGKILL)`；`kill_child_process_group` 之所以存在正是因为单 PID 不够）。这恰好预测了止血记录观察到的形状：23 个 mock 组中 18 个的组长已死而组内 `sh -c` 与 `sleep 0.05` 仍活——组长被斩首，组被孤儿化。

   本条修正带出两个原稿没有的泄漏入口：**`task delete`、步骤超时与 stall 自动 kill 同样会制造孤儿会话**，不止 daemon 死亡这一个触发条件。相应地，需求 4 不是"尽力而为的又一层"——它是目前唯一存在的优雅回收路径。

3. **协调器只改数据库，从不动 OS**。`crates/orchestrator-persistence/src/session_store.rs:429` 的 `reconcile_sessions`（由 `crates/daemon/src/main.rs:512` 每 10 秒调用）对"进程活着但 transport 已消失"的会话，只把状态改成 `failed`；对"活着且 transport 尚存"的改成 `detached`。全过程零 `kill`。判定所需的身份校验其实**已经实现**——`session_store.rs:280` 的 `process_identity_status` 已用 `process_fingerprint` 排除 PID 复用——只是结论没有被用于回收。

4. **保洁程序会删掉活体的档案**（机制属实，但**当前不可达**——2026-08-03 实施期核实）。全仓库检索 `cleanup_stale_sessions` 只得到三处定义（`session_store.rs:583` 的实现、`repository/session.rs` 的 trait 与实现、`AsyncSessionStore` 的门面）与两处测试，**零个生产调用点**。原稿称本条为"本 FR 中最尖锐的一处"，代码层面确实如此，但它描述的是一处**潜伏**缺陷而非正在发生的泄漏：这个扫除从未在生产路径上跑过。修复仍然值得做（这是公开 API，迟早会被接上），但不应把它算作已观测泄漏的成因之一。`session_store.rs:583` 的 `cleanup_stale_sessions` 删除 `state IN ('exited','closed','failed')` 且超龄的行。而第 3 点刚把一个**仍在运行**的孤儿标成 `failed`——于是系统主动抹掉了自己对该活体的唯一记录，此后再无追踪依据。这是本 FR 中最尖锐的一处：不是忘了回收，是先放弃回收再销毁证据。

触发条件在 QA 路径上是常态而非例外。`scripts/qa/test-agent-session-control-plane.sh:63` 的回收依赖 `trap cleanup EXIT`，脚本被 SIGKILL 或 CI 超时强杀时 trap 根本不执行——6 个 `ppid=1` 的 daemon 就是 trap 未执行的直接证据。

即使 trap 正常执行也回收不到会话，且原因比原稿写的更彻底（2026-08-03 治理核实）。原稿说 `cleanup`（`:50`）"只 kill 单个 `$SESSION_PROCESS_PID` 变量（`:415` 赋值，会被后续覆盖）"——该变量**只有一处赋值**（`:415`），并未被后续覆盖；而它赋的是一个合成的 `sleep 300`（用于伪造重启测试所需的会话行），**从来不是任何真实 mock 会话的 PID**。真实会话由 `:226` 的 `task start` 派生，其 PID 只存在于数据库里，脚本从未记录过。也就是说 `cleanup` 对会话进程的回收能力不是"不够"，而是**零**——全部依赖 `stop_daemon` 加 `rm -rf`，而按第 2 点，daemon 之死并不会带走会话。这加强而非削弱了需求 6。

泄漏之所以昂贵，还有一个独立成因：`fixtures/manifests/bundles/session-control-mock.yaml:20` 的循环写成 `while true; do if IFS= read -r line; then ...; else sleep 0.05; fi; done`。FIFO 写端随 tmpdir 消失后 `read` 立即返回 EOF，循环退化成 20Hz 空转。对比证据很干净：使用阻塞式 `while IFS= read -r line; do` 变体的两个进程 CPU 时间为 `0:00.00`，而轮询变体每个都是 ~315 分钟。孤儿化是漏，忙等把漏变成了持续成本。

### 第五条泄漏路径：临时目录（2026-08-03 一次性止血时发现，与前四层同源但不被其覆盖）

上述四层讲的都是进程。2026-08-03 01:00 执行一次性止血（回收 28 个 mock 会话 + 6 个孤儿 daemon，合计 135.5 小时累计 CPU）之后复查发现：**进程被回收了，它们的目录一个都没有走**。

在本机 `$TMPDIR`（`/private/var/folders/s6/.../T/`）下，持有 `agent_orchestrator.db` 的顶层目录共 **14937 个**（方法：`find "$TMPDIR" -maxdepth 3 -name agent_orchestrator.db`，去重到顶层路径分量，at 2026-08-03 01:15）。按形状精确分解，四项相加即为总数：

| 形状 | 数量 | 产生处 |
|---|---|---|
| `agent-orchestrator-test-<nanos>-<uuid>` | 10843 | `core/src/test_utils.rs:165` 的 `TestState::new()`，经由 `core/src/db_write.rs:74` 的 `mem::forget` 泄漏（见下） |
| `config-load-test-<uuid>` | 4021 | `core/src/config_load/mod.rs:184` 的 `make_test_db()` |
| `tmp.<mktemp>` | 68 | QA shell 脚本的 `mktemp -d` |
| `wp05-qa.<mktemp>` | 5 | WP05 QA 脚本 |

占用磁盘 **约 12 GB**。这是抽样估算而非全量：每形状各取前 200 个做 `du -sk` 求均值再外推，得 10843 × 845 KB ≈ 8.95 GB 与 4021 × 798 KB ≈ 3.13 GB，抽样比例分别为 1.8% 与 5.0%，**未做全量校验**。目录 mtime 最早 2026-06-28、最新 2026-08-03，即 **36 天持续累积**。

**两个大形状的成因不同，不可合并处理**：

- `make_test_db()` 返回 `(PathBuf, PathBuf)`，调用方写作 `let (_temp_dir, db_path) = make_test_db();`——绑定的是 `PathBuf`，**没有任何清理语义**。这是构造上的必然泄漏，读代码即可确认，无需实测。
- `TestState` **有** `Drop`（`test_utils.rs:468`），无条件执行 `remove_dir_all(&self.temp_root)`。原稿据此记「根因未确证，不要按『忘了加 Drop』去实施」。

  **根因已于 2026-08-03 治理核实中确证，且确实不是 Drop 失效。** 泄漏点是 `core/src/db_write.rs:74` 的 `std::mem::forget(fixture)`——写在共享的 `setup_task()` 辅助函数里，附注释「Leak fixture so the temp dir survives for the test」。它是一处**刻意的、有文档的泄漏**：`setup_task()` 返回 `Arc<InnerState>` 而不返回 fixture，若 fixture 在此处析构，临时目录会在测试用到它之前就被删掉，于是作者选择 forget 它。`setup_task()` 恰有 **38 处调用**。

  `TestState::Drop` 本身是好的，并且 `test_utils.rs:505-513` 已有一个测试证明它生效。残留目录是完整的构建产物（`secrets/`、`workspace/default/`、`logs/`、`-wal`、`-shm`），与「fixture 被 forget」一致，与「`remove_dir_all` 失败」不一致。

  **因此需求 7 第二处的「在根因确证前不应排期实施」前提已解除**，改法是让 `setup_task()` 把 fixture 一并返回、由调用方持有到测试结束，而不是补 Drop。

按天统计两个形状的数量可作为独立佐证，但原稿的表述需要修正：比值**并非「在每一天都恒定为 19:7」**。15 天中 11 天吻合，**4 天偏离**——07-17（490/184）、07-22（542/197）、07-26（1487/560）、08-02（1750/658）；原稿只列举了吻合的那几天（190/70、1520/560、1140/420、456/168、190/70）。

不过原稿从中得出的结论仍然成立，且现在有了更硬的来源：`make_test_db()` 有 **14 处调用**且构造上必然泄漏，故每轮 14 份；`setup_task()` 有 **38 处调用**，故每轮 38 份。**38 : 14 = 19 : 7**，与观测到的主流比值精确相符。这条路径是系统性的（每轮泄漏固定份数），不是中断驱动的意外——但这一点现在由代码路径确立，而不再依赖「比值恒定」这一被证伪的表述。

与前四层的关系：同一个触发条件（`trap cleanup EXIT` 未执行、进程被强杀），但**需求 1~6 没有任何一条会回收目录**。进程回收之后，目录是这场泄漏唯一剩下的证据与成本；而按需求 1 的设计，回收动作发生时协调器手里恰好握有该会话的路径。

## 需求

### 1. 协调器接上 OS 层回收（核心）

- `reconcile_sessions` 判定为 `failed`（`ProcessIdentityStatus::VerifiedLive` 且 transport 已消失）时，对该 PID 执行进程组回收，而非仅改数据库状态；
- 前置条件为 `process_fingerprint` 校验通过（复用 `process_identity_status` 的既有判定），`Mismatch` 与 `Unsupported` 一律不杀——PID 复用误杀的代价远高于泄漏。该校验须在**发信号前紧邻处重新执行**，而不是复用协调轮次里算出的旧结论，以关掉 TOCTOU 窗口；
- **另需校验 `getpgid(pid) == pid`**（2026-08-03 治理核实补入）。fingerprint 证明的是**身份**，不是**组长身份**；若记录的 pid 恰好不是自己那个组的组长，`kill(-pid)` 会打到一个无关的进程组上。这是一个代理条件冒充另一个属性（技能 §4.4 shape 1），必须单独校验、单独出负向 fixture；
- 回收动作发 `kill(-pid, SIGKILL)` 而非 `kill(pid, ...)`：会话自身可能已派生子进程，只杀组长会制造新一代孤儿；
- 受 `RuntimePolicy` 的 `session_reclaim_enabled` 开关控制，**默认为 true**。刻意不复用既有的 `session_control_enabled`：后者默认为 **false**，若挂在它下面，默认配置里本 P1 修复将永不执行——那会造出一个为自己没做的事发绿灯的门禁；
- 每次回收发出事件（session id、pid、fingerprint、判定依据），使回收行为本身可审计、可在 QA 中断言。

### 2. `cleanup_stale_sessions` 拒绝删除活体档案

- 删除前对每行做一次存活探测，`process_exists` 为真则跳过删除并计入告警；
- 语义修正为"只销毁已确认死亡的记录"——保洁不得成为失忆机制。

### 3. `AgentSessionClose` 改用进程组回收

- `crates/daemon/src/server/session.rs:939` 当前发 `SIGTERM` 到 `row.pid` 单进程。`spawn.rs:69` 特意让会话自成进程组正是为了让组回收成为可能，close 路径没有用上这一点；
- 改为对进程组发信号，并保留现有的 `draining` 状态与失败回滚语义。

### 4. daemon 关停时排空会话

- `crates/daemon/src/main.rs` 的关停序列在 `shutdown_running_tasks` 之后，对数据库中所有非终态会话执行一次回收（复用需求 1 的原语），采用 SIGTERM → 等待 → SIGKILL 的升级式信号（止血记录里 29 个组全部 SIGTERM 即退，无一需要 SIGKILL）；
- 明确这是尽力而为的一层：SIGKILL 下不会执行，因此需求 1 的周期性协调才是兜底。
- **修正原稿的定位**：按背景第 2 点的核实结论，这不是"又一层"——由于会话子进程从未进入 `runtime.child`，`shutdown_running_tasks` 对会话完全无效，本条是目前**唯一**存在的优雅回收路径。

### 5. fixture 忙等改为阻塞读

- `session-control-mock.yaml` 的会话循环改为阻塞式 `read`。泄漏修复后本项不再影响正确性，但它决定了"一旦仍有泄漏，代价是 0 还是 133 小时 CPU"。

### 6. QA 脚本回收不再依赖 EXIT trap

- `test-agent-session-control-plane.sh` 记录本次运行派生的**全部**会话 PID（而非单变量），并在启动时清理上一轮同 fixture 的残留；
- trap 保留，但不再是唯一防线。

### 7. 临时目录回收（与需求 1~6 同源但独立，不要合并进任一条）

分三处，各自的判据不同：

- **`make_test_db()` 改为返回持有清理语义的类型**（`tempfile::TempDir`；仓库内已有大量先例：`core/src/action_audit.rs`、`core/src/source_connection.rs`、`core/src/handoff.rs`、`crates/orchestrator-persistence/src/attention_store.rs`）。调用方的 `let (_temp_dir, db_path) = ...` 形态无需改动，绑定类型变为 `TempDir` 后即在测试结束时自动删除。这条是构造性修复，读代码即可判定完成。
- **删掉 `core/src/db_write.rs:74` 的 `std::mem::forget(fixture)`。** 原稿在此写「在根因确证之前不要动这段代码」——根因已于 2026-08-03 确证（见上文第五条泄漏路径），前提解除。`TestState` 的 `Drop` 一直是好的；泄漏来自 `setup_task()` 刻意 forget 掉 fixture，因为它只返回 `Arc<InnerState>`，若就地析构则临时目录会先于测试被删。改法是让 `setup_task()` 把 fixture 一并返回、由 38 个调用方各自持有到测试结束，**而不是补 Drop**。
- **协调器回收会话时一并清理其目录**（需求 1 的延长线）。前置条件与需求 1 相同：仅在 `process_fingerprint` 校验通过后执行，且删除范围严格限于该会话自己的 `logs/sessions/<session_id>/`，**不得**上溯删除整个 `data/` 或 tmpdir——那会波及同一 daemon 下的其它会话。

**明确不做**：不新增「扫描 `$TMPDIR` 删除超龄目录」的后台清道夫。那是把一个泄漏换成一个持有删除权限的定时任务，其判据只有文件名与 mtime，而 CLAUDE.md 的第一条禁令正是不得删除数据库——一个按 glob 匹配删目录的进程，离误删只差一个前缀。修复点在产生处，不在清扫处。

## 验收标准

- [x] ~~负向验证：启动一个 interactive session 后对 daemon 发 `SIGKILL`，重启 daemon，在两个协调周期内该会话进程被回收且发出回收事件~~ **本条与 DD-112 冲突，已撤销**（2026-08-03 实施期核实，见下）
- [ ] 回收判据改为**transport 消失**（FR 需求 1 自己写的规则）：会话进程存活但其 FIFO 已不存在时，在两个协调周期内被回收并发出 `session_process_reclaimed` 事件
- [ ] **该断言的反证 fixture**：同一场景下置 `session_reclaim_enabled: false`，会话进程在两个协调周期后**仍然存活**。没有这一条，上一条在「本机当前恰好没有孤儿」时是恒真的（见下方环境注记）

#### 撤销第一条判据的理由（2026-08-03）

实施「daemon 死后其会话即孤儿」的判据（按 ppid 是否为当前 daemon 判断）会**删掉一个既有且有意为之的特性**。DD-112 §54 明确规定重启协调把「进程存活 + transport 存在」收敛到 `active`/`detached`/`draining`，§35 把「daemon 运行时拆除后仍保持 orchestrator 所属子进程存活」列为设计属性；QA 149 场景 5 断言重启后会话保持 `detached` 且**可再附着**。这不是疏漏：FIFO 是磁盘上的具名管道，输出捕获也是文件，所以新 daemon 确实能继续驱动这个会话。

实测证据：按 ppid 判据实施后，`test-agent-session-control-plane.sh` 的重启场景以 `session is not attachable` 失败——协调器把那个本该 `detached` 的会话标成 `failed` 并把它杀了。

**本 FR 原稿的第一条验收判据与 DD-112 的重启契约直接矛盾**，且原稿未察觉。保留的是 FR 需求 1 自己写的规则（transport 消失），它恰好覆盖了真实观测到的泄漏：那 28 个 mock 会话的临时目录已被 `rm -rf`，FIFO 随之消失。代价是「daemon 被 SIGKILL 且数据目录完好」这一残余情形仍不由协调器回收——那正是 DD-112 有意保留的可恢复会话，由需求 4（关停排空）与需求 6（QA 脚本回收）覆盖其余入口。
- [ ] 负向验证：构造 `process_fingerprint` 不匹配的记录（模拟 PID 复用），协调器不发送任何信号，且不将该 PID 记为已回收；同一 PID 换成匹配的 fingerprint 后被回收（证明该拒绝不是恒拒）
- [ ] 负向验证：记录的 pid 不是自身进程组组长时，回收被拒绝并留下点名该原因的诊断（`getpgid(pid) != pid`）
- [ ] 会话派生的孙进程随会话一同消失（进程组回收生效，而非仅组长退出）；反证：单 PID kill 的变体会留下该孙进程
- [ ] `cleanup_stale_sessions` 在存在活体 `failed` 行时跳过删除并留下告警记录；反证：同形状但 PID 已死的行仍被删除
- [ ] `AgentSessionClose` 关闭一个有子进程的会话后，子进程不残留
- [ ] `test-agent-session-control-plane.sh` 在被 `SIGKILL` 中断后重跑，不累积残留进程（连续两轮后 `ppid=1` 的 mock 进程数为 0）。该检查**必须对空输入 fail-closed**：`ps` 读到零行与「零个孤儿」在退出码上无法区分（技能 §4.4 shape 5）
- [ ] 全量 QA sweep 结束后 `ps` 中不存在 `ppid=1` 的 `orchestratord` 与 fixture 会话进程
- [ ] 跑一次 `cargo test`，前后 `config-load-test-*` 的计数差为 0。计数须在**该次运行私有的 `TMPDIR`** 下进行，而不是共享的 `$TMPDIR`——后者与机器上任何其它活动竞态
- [ ] 同一次运行前后 `agent-orchestrator-test-*` 的计数差为 0；根因（`db_write.rs:74` 的 `mem::forget`，非 Drop 失效）写入 DD
- [ ] 负向验证：临时移除 `make_test_db` 的清理语义后，上述计数差 > 0（证明该断言不是恒真）
- [ ] **活性断言**：上述计数差检查须同时断言该次测试运行确实执行了 N 个测试。一次根本没跑起来的测试运行同样给出计数差 0
- [ ] 协调器回收一个会话后，其 `logs/sessions/<session_id>/` 消失，而同一 daemon 下其它会话的目录与 `data/agent_orchestrator.db` 均不受影响

### 环境注记（2026-08-03 治理核实）

本机当前**孤儿数为 0**——无 `ppid=1` 的 `orchestratord`，19xxx 端口无 LISTEN，止血成果保持住了。因此上面两条「`ps` 中不存在」的判据**在今天不改任何代码也为真**，单靠观测无法证明修复生效。它们必须由各自的反证 fixture 支撑，否则认证的是本机当前状态而不是这次改动。

## 依赖与关联

- 与 FR-157（Source 域分解与测试补强）、FR-158（治理体系自省）无实现耦合，可并行。相对二者应优先：这是持续泄漏资源的运行时缺陷，而非结构债。
- 关联 FR-033（Daemon 重启后孤立 Running Items 自动恢复）——那次解决的是**数据库侧**的孤儿恢复，本 FR 是同一场景在**进程侧**未被覆盖的另一半；两者共享"daemon 非优雅退出"这一触发条件。
- 关联 FR-040 / FR-046（Agent 子进程 Daemon PID Guard 穿透防护）：同属子进程生命周期治理，方向相反——那两个防止子进程杀死 daemon，本 FR 处理 daemon 死后子进程不死。
- 验证载体应落在 `docs/qa/orchestrator/` 既有的 145/149 两篇 session control plane QA 文档的延长线上，而非新开一条。注意 `docs/qa/orchestrator/159-*` 编号已被 source-automation-reliability-operations 占用，本 FR 的 QA 文档取下一个空号（209），DD 取 171——FR 编号与文档编号在本仓库不同步。
- 需求 7 的第一处（`make_test_db`）与需求 1~6 无耦合，可先行落地；第二处的排期前提（根因确证）已于 2026-08-03 解除，见需求 7 正文。

## 一次性止血记录（2026-08-03，非修复）

本 FR 立项前的存量已在 2026-08-03 01:00 手工回收：28 个 mock 会话进程（`ppid=1`，或其 `ppid` 为同样孤儿化的 wrapper）与 6 个孤儿 `orchestratord`，共 29 个进程组，全部 SIGTERM 即退、无一需要 SIGKILL、事后无僵尸；端口 19394–19399 全部释放；回收累计 CPU **135.5 小时**（mock 135.4 h + daemon 0.14 h）。

判别器按 DB 路径逐个核实：6 个 daemon 持有的库全部位于 `$TMPDIR/tmp.XXXXXX/data/`，无一位于 `~/.orchestratord/`——该目录在本机根本不存在，故无真实 daemon 需要跳过。

两处值得留档的操作事实。其一，信号发往**进程组**而非 PID：23 个 mock 组中 18 个的组长（bash wrapper）已先行死亡，组内实际存活的是 `sh -c` 本体与其派生的 `sleep 0.05`——只杀组长收不干净，这正是 `spawn.rs:69` 让会话自成进程组的用意。其二，止血前枚举了每个目标组的全部成员以确认无无关进程共享 pgid；第一版枚举脚本在 zsh 下写成 `for g in $PGIDS`，而 zsh 不对未加引号的参数做分词，于是它把整串当作一个 pgid 检查了一遍，然后报告零个成员、全部合规——一次读了零输入的 PASS，即技能 §4.4 shape 5 的形状。改用 bash 重跑才拿到真实结果。

止血不改变本 FR 的优先级：泄漏在下一次 QA 运行时即重新开始。
