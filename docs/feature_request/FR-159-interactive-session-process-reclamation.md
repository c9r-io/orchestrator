# FR-159: 交互会话进程回收 — 孤儿泄漏与 OS 层回收缺口

## 优先级: P1

## 状态: Proposed

## 背景

2026-08-02 在开发机上的实测：**28 个 `session-control-mock` 会话进程存活最长 19 天，其中 23 个已被 reparent 到 `init`（`ppid=1`）；另有 6 个 `./target/debug/orchestratord` 同样 `ppid=1`，端口 19394–19399 仍在 LISTEN**。这些不是僵尸进程——全系统 `Z` 状态计数为 0——而是活着的孤儿，且在持续消耗 CPU：当前合计 28.1%（约 0.28 核），累计已烧掉 **133.7 小时 CPU 时间**。

泄漏路径由四层缺口叠加而成，每一层单独看都是合理设计：

1. **子进程自成进程组，因而免疫于守护进程之死**。`crates/orchestrator-runner/src/runner/spawn.rs:69` 用 `process_group(0)` 让每个子进程成为组长，这是 `kill_child_process_group`（同文件 `:187`，发 `kill(-pid, SIGKILL)`）能够连坐整棵子树的前提。代价是：daemon 被 SIGKILL 或崩溃时，信号不会波及这些独立进程组。

2. **`kill_on_drop(true)` 与 `shutdown_running_tasks` 都覆盖不到 interactive session**。`spawn.rs:60` 的 `kill_on_drop` 只在优雅退出走 `Drop` 时生效；`crates/orchestrator-scheduler/src/scheduler/runtime.rs:163` 的 `shutdown_running_tasks` 只遍历 `state.running`——即**在途任务步骤**——而持久化的 interactive session 不在该集合内。两条既有回收路径的交集不覆盖会话进程。

3. **协调器只改数据库，从不动 OS**。`crates/orchestrator-persistence/src/session_store.rs:429` 的 `reconcile_sessions`（由 `crates/daemon/src/main.rs:512` 每 10 秒调用）对"进程活着但 transport 已消失"的会话，只把状态改成 `failed`；对"活着且 transport 尚存"的改成 `detached`。全过程零 `kill`。判定所需的身份校验其实**已经实现**——`session_store.rs:280` 的 `process_identity_status` 已用 `process_fingerprint` 排除 PID 复用——只是结论没有被用于回收。

4. **保洁程序会删掉活体的档案**。`session_store.rs:583` 的 `cleanup_stale_sessions` 删除 `state IN ('exited','closed','failed')` 且超龄的行。而第 3 点刚把一个**仍在运行**的孤儿标成 `failed`——于是系统主动抹掉了自己对该活体的唯一记录，此后再无追踪依据。这是本 FR 中最尖锐的一处：不是忘了回收，是先放弃回收再销毁证据。

触发条件在 QA 路径上是常态而非例外。`scripts/qa/test-agent-session-control-plane.sh:63` 的回收依赖 `trap cleanup EXIT`，脚本被 SIGKILL 或 CI 超时强杀时 trap 根本不执行——6 个 `ppid=1` 的 daemon 就是 trap 未执行的直接证据。即使 trap 正常执行，`cleanup`（`:50`）也只 kill 单个 `$SESSION_PROCESS_PID` 变量（`:415` 赋值，会被后续覆盖），本就管不住一次运行里的多个会话。

泄漏之所以昂贵，还有一个独立成因：`fixtures/manifests/bundles/session-control-mock.yaml:18` 的循环写成 `while true; do if IFS= read -r line; then ...; else sleep 0.05; fi; done`。FIFO 写端随 tmpdir 消失后 `read` 立即返回 EOF，循环退化成 20Hz 空转。对比证据很干净：使用阻塞式 `while IFS= read -r line; do` 变体的两个进程 CPU 时间为 `0:00.00`，而轮询变体每个都是 ~315 分钟。孤儿化是漏，忙等把漏变成了持续成本。

### 第五条泄漏路径：临时目录（2026-08-03 一次性止血时发现，与前四层同源但不被其覆盖）

上述四层讲的都是进程。2026-08-03 01:00 执行一次性止血（回收 28 个 mock 会话 + 6 个孤儿 daemon，合计 135.5 小时累计 CPU）之后复查发现：**进程被回收了，它们的目录一个都没有走**。

在本机 `$TMPDIR`（`/private/var/folders/s6/.../T/`）下，持有 `agent_orchestrator.db` 的顶层目录共 **14937 个**（方法：`find "$TMPDIR" -maxdepth 3 -name agent_orchestrator.db`，去重到顶层路径分量，at 2026-08-03 01:15）。按形状精确分解，四项相加即为总数：

| 形状 | 数量 | 产生处 |
|---|---|---|
| `agent-orchestrator-test-<nanos>-<uuid>` | 10843 | `core/src/test_utils.rs:170` 的 `TestState::new()` |
| `config-load-test-<uuid>` | 4021 | `core/src/config_load/mod.rs:186` 的 `make_test_db()` |
| `tmp.<mktemp>` | 68 | QA shell 脚本的 `mktemp -d` |
| `wp05-qa.<mktemp>` | 5 | WP05 QA 脚本 |

占用磁盘 **约 12 GB**。这是抽样估算而非全量：每形状各取前 200 个做 `du -sk` 求均值再外推，得 10843 × 845 KB ≈ 8.95 GB 与 4021 × 798 KB ≈ 3.13 GB，抽样比例分别为 1.8% 与 5.0%，**未做全量校验**。目录 mtime 最早 2026-06-28、最新 2026-08-03，即 **36 天持续累积**。

**两个大形状的成因不同，不可合并处理**：

- `make_test_db()` 返回 `(PathBuf, PathBuf)`，调用方写作 `let (_temp_dir, db_path) = make_test_db();`——绑定的是 `PathBuf`，**没有任何清理语义**。这是构造上的必然泄漏，读代码即可确认，无需实测。
- `TestState` **有** `Drop`（`test_utils.rs:468`），无条件执行 `remove_dir_all(&self.temp_root)`。所以它的 10843 份残留意味着 Drop 没有执行，或目录在 Drop 之后被重建。**根因未确证**，不要按「忘了加 Drop」去实施。

有一项证据把方向收窄了：按天统计两个形状的数量，比值在每一天都恒定为 **19:7**（190/70、1520/560、1140/420、456/168、190/70……）。恒定比值意味着**每次 `cargo test` 运行泄漏固定份数**，而不是「偶尔被中断时才泄漏」——若是中断驱动，比值会随中断时机浮动。这条路径是系统性的，不是意外。

与前四层的关系：同一个触发条件（`trap cleanup EXIT` 未执行、进程被强杀），但**需求 1~6 没有任何一条会回收目录**。进程回收之后，目录是这场泄漏唯一剩下的证据与成本；而按需求 1 的设计，回收动作发生时协调器手里恰好握有该会话的路径。

## 需求

### 1. 协调器接上 OS 层回收（核心）

- `reconcile_sessions` 判定为 `failed`（`ProcessIdentityStatus::VerifiedLive` 且 transport 已消失）时，对该 PID 执行进程组回收，而非仅改数据库状态；
- 前置条件为 `process_fingerprint` 校验通过（复用 `process_identity_status` 的既有判定），`Mismatch` 与 `Unsupported` 一律不杀——PID 复用误杀的代价远高于泄漏；
- 回收动作发 `kill(-pid, SIGKILL)` 而非 `kill(pid, ...)`：会话自身可能已派生子进程，只杀组长会制造新一代孤儿；
- 每次回收发出事件（session id、pid、fingerprint、判定依据），使回收行为本身可审计、可在 QA 中断言。

### 2. `cleanup_stale_sessions` 拒绝删除活体档案

- 删除前对每行做一次存活探测，`process_exists` 为真则跳过删除并计入告警；
- 语义修正为"只销毁已确认死亡的记录"——保洁不得成为失忆机制。

### 3. `AgentSessionClose` 改用进程组回收

- `crates/daemon/src/server/session.rs:939` 当前发 `SIGTERM` 到 `row.pid` 单进程。`spawn.rs:69` 特意让会话自成进程组正是为了让组回收成为可能，close 路径没有用上这一点；
- 改为对进程组发信号，并保留现有的 `draining` 状态与失败回滚语义。

### 4. daemon 关停时排空会话

- `crates/daemon/src/main.rs` 的关停序列在 `shutdown_running_tasks` 之后，对数据库中所有非终态会话执行一次回收（复用需求 1 的原语）；
- 明确这是尽力而为的一层：SIGKILL 下不会执行，因此需求 1 的周期性协调才是兜底。

### 5. fixture 忙等改为阻塞读

- `session-control-mock.yaml` 的会话循环改为阻塞式 `read`。泄漏修复后本项不再影响正确性，但它决定了"一旦仍有泄漏，代价是 0 还是 133 小时 CPU"。

### 6. QA 脚本回收不再依赖 EXIT trap

- `test-agent-session-control-plane.sh` 记录本次运行派生的**全部**会话 PID（而非单变量），并在启动时清理上一轮同 fixture 的残留；
- trap 保留，但不再是唯一防线。

### 7. 临时目录回收（与需求 1~6 同源但独立，不要合并进任一条）

分三处，各自的判据不同：

- **`make_test_db()` 改为返回持有清理语义的类型**（`tempfile::TempDir`；仓库内已有大量先例：`core/src/action_audit.rs`、`core/src/source_connection.rs`、`core/src/handoff.rs`、`crates/orchestrator-persistence/src/attention_store.rs`）。调用方的 `let (_temp_dir, db_path) = ...` 形态无需改动，绑定类型变为 `TempDir` 后即在测试结束时自动删除。这条是构造性修复，读代码即可判定完成。
- **先查清 `TestState` 的 `Drop` 为何没有生效，再决定改法。** 它已经写了 `remove_dir_all`，所以「补一个 Drop」是错误方向。恒定 19:7 的比值说明每次运行泄漏固定份数，据此可用一次受控的单包 `cargo test` 前后计数差把范围收敛到具体测试。**在根因确证之前不要动这段代码**——DD-170 的教训是同一条：把「今天这棵树恰好如此」当成「检查在做什么」。
- **协调器回收会话时一并清理其目录**（需求 1 的延长线）。前置条件与需求 1 相同：仅在 `process_fingerprint` 校验通过后执行，且删除范围严格限于该会话自己的 `logs/sessions/<session_id>/`，**不得**上溯删除整个 `data/` 或 tmpdir——那会波及同一 daemon 下的其它会话。

**明确不做**：不新增「扫描 `$TMPDIR` 删除超龄目录」的后台清道夫。那是把一个泄漏换成一个持有删除权限的定时任务，其判据只有文件名与 mtime，而 CLAUDE.md 的第一条禁令正是不得删除数据库——一个按 glob 匹配删目录的进程，离误删只差一个前缀。修复点在产生处，不在清扫处。

## 验收标准

- [ ] 负向验证：启动一个 interactive session 后对 daemon 发 `SIGKILL`，重启 daemon，在两个协调周期内该会话进程被回收且发出回收事件
- [ ] 负向验证：构造 `process_fingerprint` 不匹配的记录（模拟 PID 复用），协调器不发送任何信号，且不将该 PID 记为已回收
- [ ] 会话派生的孙进程随会话一同消失（进程组回收生效，而非仅组长退出）
- [ ] `cleanup_stale_sessions` 在存在活体 `failed` 行时跳过删除并留下告警记录
- [ ] `AgentSessionClose` 关闭一个有子进程的会话后，子进程不残留
- [ ] `test-agent-session-control-plane.sh` 在被 `SIGKILL` 中断后重跑，不累积残留进程（连续两轮后 `ppid=1` 的 mock 进程数为 0）
- [ ] 全量 QA sweep 结束后 `ps` 中不存在 `ppid=1` 的 `orchestratord` 与 fixture 会话进程
- [ ] 跑一次 `cargo test --workspace`，前后 `$TMPDIR` 下 `config-load-test-*` 的计数差为 0
- [ ] 同一次运行前后 `agent-orchestrator-test-*` 的计数差为 0；若根因确证为"Drop 不执行"以外的机制，在 DD 中写明该机制而非仅记结果
- [ ] 负向验证：临时移除 `make_test_db` 的清理语义后，上述计数差 > 0（证明该断言不是恒真）
- [ ] 协调器回收一个会话后，其 `logs/sessions/<session_id>/` 消失，而同一 daemon 下其它会话的目录与 `data/agent_orchestrator.db` 均不受影响

## 依赖与关联

- 与 FR-157（Source 域分解与测试补强）、FR-158（治理体系自省）无实现耦合，可并行。相对二者应优先：这是持续泄漏资源的运行时缺陷，而非结构债。
- 关联 FR-033（Daemon 重启后孤立 Running Items 自动恢复）——那次解决的是**数据库侧**的孤儿恢复，本 FR 是同一场景在**进程侧**未被覆盖的另一半；两者共享"daemon 非优雅退出"这一触发条件。
- 关联 FR-040 / FR-046（Agent 子进程 Daemon PID Guard 穿透防护）：同属子进程生命周期治理，方向相反——那两个防止子进程杀死 daemon，本 FR 处理 daemon 死后子进程不死。
- 验证载体应落在 `docs/qa/orchestrator/` 既有的 145/149 两篇 session control plane QA 文档的延长线上，而非新开一条。
- 需求 7 的第一处（`make_test_db`）与需求 1~6 无耦合，可先行落地；第二处（`TestState` 的 Drop）在根因确证前不应排期实施。

## 一次性止血记录（2026-08-03，非修复）

本 FR 立项前的存量已在 2026-08-03 01:00 手工回收：28 个 mock 会话进程（`ppid=1`，或其 `ppid` 为同样孤儿化的 wrapper）与 6 个孤儿 `orchestratord`，共 29 个进程组，全部 SIGTERM 即退、无一需要 SIGKILL、事后无僵尸；端口 19394–19399 全部释放；回收累计 CPU **135.5 小时**（mock 135.4 h + daemon 0.14 h）。

判别器按 DB 路径逐个核实：6 个 daemon 持有的库全部位于 `$TMPDIR/tmp.XXXXXX/data/`，无一位于 `~/.orchestratord/`——该目录在本机根本不存在，故无真实 daemon 需要跳过。

两处值得留档的操作事实。其一，信号发往**进程组**而非 PID：23 个 mock 组中 18 个的组长（bash wrapper）已先行死亡，组内实际存活的是 `sh -c` 本体与其派生的 `sleep 0.05`——只杀组长收不干净，这正是 `spawn.rs:69` 让会话自成进程组的用意。其二，止血前枚举了每个目标组的全部成员以确认无无关进程共享 pgid；第一版枚举脚本在 zsh 下写成 `for g in $PGIDS`，而 zsh 不对未加引号的参数做分词，于是它把整串当作一个 pgid 检查了一遍，然后报告零个成员、全部合规——一次读了零输入的 PASS，即技能 §4.4 shape 5 的形状。改用 bash 重跑才拿到真实结果。

止血不改变本 FR 的优先级：泄漏在下一次 QA 运行时即重新开始。
