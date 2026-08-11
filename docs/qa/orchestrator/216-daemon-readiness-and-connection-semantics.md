---
lifecycle: active
related_fr: FR-163
self_referential_safe: true
---

# QA-216: 就绪信号与连接语义

FR-163 需求 3、4 的验证。需求 1、2 见 [QA-215](215-connectivity-path-single-source.md)。

门禁使用独立的 `mktemp` 数据目录与独立的 `HOME`，不触碰开发者自己的 daemon、
数据库或 `~/.orchestratord`（QA §4.7）。

## 背景：本轮修掉的是什么

**就绪信号不存在。** 24 处轮询散在 23 个门禁里，全是同样五行的手抄件，
且对超时预算各执一词：7.5s、10s、15s、20s、25s，没有一个是推导出来的。
两个比数字更重的问题：

1. `task list` 是**代理指标**。它在套接字接受连接的那一刻就成功，而 worker
   supervisor 虽然在 bind 前约 560 行就已 spawn，每个 worker 却是**异步注册**的
   ——门禁可以创建出任务，然后眼看着没有人来领。
2. `&& break` 在循环耗尽时**静默继续**。门禁于是拿一个根本没起来的 daemon
   跑完整个正文，在更下游的某处失败，指认错误的对象。

**文档缺三个主题。** `ORCHESTRATOR_SOCKET` 在 `docs/guide/` 出现 **0 次**；
`--bind` 只有一行"默认：Unix 套接字"，读起来像叠加而非互斥；陈旧 socket 的自救
无处可查。且 quickstart 第 3 步"`init` 创建 SQLite 表结构"**作为陈述即为假**。

## S1：就绪信号的行为契约（门禁脚本）

**步骤**

```bash
cargo build -p orchestratord -p orchestrator-cli
bash scripts/qa/test-daemon-readiness.sh
```

**预期**：`FR-163 daemon readiness: 10 passed, 0 failed`，退出码 0。

四条互不替代的断言：

| 断言 | 为什么它不能被其它条覆盖 |
| --- | --- |
| 等待**真的在等** | waiter 先启动、daemon 延迟 3s 才起，按**耗时**判定。立刻返回的调用在一个恰好已就绪的 daemon 上同样"成功" |
| 就绪不来时**有界失败**并指名最后所见 | 永远挂着和退出 0 都比红更糟 |
| 每个子系统**无论就绪与否都具名** | 只列失败项的报告在列表为空时读起来像完整的，"就绪"于是与"什么都没测"无法区分 |
| 不带旗标的 `daemon status` **不建立连接** | 这正是它能对一个无法服务的 daemon 作答的性质 |

**worker 计数被单独断言**（`workers=ready (2/2 started)`）——这是套接字探测**看不见**
的那个事实，也是本需求存在的理由。

**负夹具（已实测，两个方向）**：

| 变异 | 结果 |
| --- | --- |
| CLI 把连接失败当作就绪 | `3 passed, 7 failed` |
| 报告只列出未就绪的子系统 | `6 passed, 4 failed`，且日志里能看到 `orchestratord is ready ()` ——空报告读作成功 |

**不在此断言的**：`Health` 的 RBAC 层级。它必须是 ReadOnly，否则没有任何门禁调得动；
这一点由角色表单测 `required_role_mapping_is_stable` 钉住，那里能看到
**未映射分支默认 Admin**——漏登记的 RPC 不会报错，它会拿到那个让就绪探针对所有
需要它的调用方都失效的角色。测试因此同时断言 `Health` 与一个不存在的 RPC
**角色不同**，否则"等于 ReadOnly"在默认值恰好是 ReadOnly 时也会通过。

## S2：24 处轮询已全部收编（派生检查）

**步骤**

```bash
ruby - <<'RUBY'
n = 0
Dir.glob('scripts/**/*.sh').each do |f|
  lines = File.readlines(f)
  lines.each_with_index do |l, i|
    next unless l =~ /for _ in \{1\.\.\d+\}; do/
    next unless lines[i+1, 3].to_a.join =~ /task list[^\n]*>\s*\/dev\/null/
    n += 1; puts "#{f}:#{i+1}"
  end
end
puts "remaining hand-written task-list readiness loops: #{n}"
RUBY
rg -c 'gate_daemon_wait_ready' scripts/qa/*.sh | wc -l
```

**预期**：残留 0 处；`gate_daemon_wait_ready` 调用点 23 个，
另有 1 处直接调用 `daemon status --wait-ready`（`test-slack-reaction-task-routing.sh`
需要携带传输覆盖，去探测与 TCP daemon 共用数据目录的那个 UDS 实例）。合计 24。

**两处刻意保留差异的站点**，迁移时没有被抹平：
`test-failure-visibility.sh` 另外等待磁盘上的密钥文件（就绪报告的是从数据库加载的
keyring，而该门禁后续断言读的是文件）；上述传输覆盖站点。

## S3：迁移后的门禁仍然全绿（抽样）

**步骤**

```bash
for g in test-attention-inbox test-source-task-binding test-process-timeline test-failure-visibility; do
  bash "scripts/qa/$g.sh" > "/tmp/$g.log" 2>&1; echo "$g rc=$?"
done
```

**预期**：四者 rc=0，覆盖迁移涉及的三种形状（`&& break`、函数内 `return 0`、
`if … then break`）与一处带额外条件的站点。

> **计量陷阱，记录在此**：初次执行时写成
> `echo "$(basename $g): rc=$?"`——`$(basename …)` 的命令替换会先执行并**重置
> `$?`**，于是读到的永远是 0。这正是 §4.6.4 "直接捕获退出码"的要求所防的东西，
> 而它当时正作用在认证者自己身上。退出码必须在命令之后**立即**捕获到变量里。

## S4：连接语义文档（人工核对 + 门禁）

**步骤**

```bash
bash scripts/qa/test-cli-doc-parity.sh
bash scripts/qa/test-cli-doc-parity.sh --fixture-test
bash scripts/qa-doc-lint.sh
rg -c 'ORCHESTRATOR_SOCKET' docs/guide/07-cli-reference.md docs/guide/zh/07-cli-reference.md
```

**预期**：门禁全绿；`ORCHESTRATOR_SOCKET` 在 EN/ZH 参考中各出现 ≥1 次
（治理前为 **0**）。人工核对三个主题在 EN 与 ZH 同时存在：

- `ORCHESTRATOR_SOCKET` 的含义，以及**第 1 步刻意不做连接探测**的理由；
- `--bind` 与 UDS **互斥**（不是叠加），标志表那一行也已改写；
- 发现顺序 1–5 表格，与陈旧 socket 的自救（启动新 daemon 即可，它 bind 时自清理）。

## S5：quickstart 的假陈述已订正（EN+ZH）

**步骤**

```bash
rg -n 'creates the SQLite schema|创建 SQLite 表结构' docs/guide/01-quickstart.md docs/guide/zh/01-quickstart.md
rg -n '^## (Step|第.步)' docs/guide/01-quickstart.md docs/guide/zh/01-quickstart.md
```

**预期**：不再有任何一处把 `init` 说成创建表结构的步骤；创建表结构归于**第 2 步
启动 daemon**；原第 3 步替换为 `daemon status --wait-ready`，并保留一段说明
**为什么旧写法是错的**（`init` 是发往运行中 daemon 的 RPC，daemon 不存在时跑不起来，
存在时早已迁移完）。两语言步骤编号连续，均为 1–7。

## 已知边界

- `Health` 聚合既有的 `DbStatus`/`SecretKeyStatus`/`WorkerStatus`，**自身不测量任何东西**。
  某个子系统读不出来时报告为未就绪并把错误放进 detail，**不会**变成 RPC 失败——
  调用方问的是"你能服务了吗"，用传输错误回答会让"keyring 读不出来"与"daemon 没了"
  无法区分。
- `configured_workers == 0` 按定义即就绪（只读 daemon 是操作者的正当选择），
  否则那会变成一个永远等不到头的等待。
- 门禁不覆盖 TLS 之后的 RBAC 语义（QA-58 承担），也不覆盖 `Health` 在 TCP 传输下的
  授权路径（角色由单测钉住）。

## 检查清单

- [ ] S1 `bash scripts/qa/test-daemon-readiness.sh` 报 `10 passed, 0 failed`
- [ ] S2 残留手抄轮询为 0，收编点合计 24
- [ ] S3 抽样门禁四者 rc=0（退出码立即捕获，勿放进命令替换里）
- [ ] S4 cli-doc-parity 及其夹具、qa-doc-lint 全绿；EN/ZH 三主题齐备
- [ ] S5 两语言 quickstart 无假陈述、步骤编号连续
- [ ] 新增 CI step 时同步：`OUTCOMES` 聚合行、`qa-gate-surface.json` 的 `shape`、
      `ci-step-cost.json` 的 `pendingMeasurement`
