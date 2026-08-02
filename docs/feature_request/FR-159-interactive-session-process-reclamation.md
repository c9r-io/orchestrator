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

## 验收标准

- [ ] 负向验证：启动一个 interactive session 后对 daemon 发 `SIGKILL`，重启 daemon，在两个协调周期内该会话进程被回收且发出回收事件
- [ ] 负向验证：构造 `process_fingerprint` 不匹配的记录（模拟 PID 复用），协调器不发送任何信号，且不将该 PID 记为已回收
- [ ] 会话派生的孙进程随会话一同消失（进程组回收生效，而非仅组长退出）
- [ ] `cleanup_stale_sessions` 在存在活体 `failed` 行时跳过删除并留下告警记录
- [ ] `AgentSessionClose` 关闭一个有子进程的会话后，子进程不残留
- [ ] `test-agent-session-control-plane.sh` 在被 `SIGKILL` 中断后重跑，不累积残留进程（连续两轮后 `ppid=1` 的 mock 进程数为 0）
- [ ] 全量 QA sweep 结束后 `ps` 中不存在 `ppid=1` 的 `orchestratord` 与 fixture 会话进程

## 依赖与关联

- 与 FR-157（Source 域分解与测试补强）、FR-158（治理体系自省）无实现耦合，可并行。相对二者应优先：这是持续泄漏资源的运行时缺陷，而非结构债。
- 关联 FR-033（Daemon 重启后孤立 Running Items 自动恢复）——那次解决的是**数据库侧**的孤儿恢复，本 FR 是同一场景在**进程侧**未被覆盖的另一半；两者共享"daemon 非优雅退出"这一触发条件。
- 关联 FR-040 / FR-046（Agent 子进程 Daemon PID Guard 穿透防护）：同属子进程生命周期治理，方向相反——那两个防止子进程杀死 daemon，本 FR 处理 daemon 死后子进程不死。
- 验证载体应落在 `docs/qa/orchestrator/` 既有的 145/149 两篇 session control plane QA 文档的延长线上，而非新开一条。
