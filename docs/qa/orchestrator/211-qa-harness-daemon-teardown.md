---
lifecycle: active
related_fr: FR-160
self_referential_safe: true
---

# QA 211: QA harness 守护进程拆解（gate_daemon.sh）

验证 FR-160：25 个 QA 门禁的守护进程停止全部经由 `scripts/lib/gate_daemon.sh`，
`wait` 对 pidfile PID 的空操作被真实等待取代，且 check 16 阻止第 26 个偏离站点。

所有场景只读或使用隔离的临时目录，不触碰运行中的 orchestratord 或其数据库。

## 场景 1：复现探针（两种形状并排断言）

**Steps**

```bash
bash scripts/qa/probe-daemon-wait-shapes.sh; echo "exit=$?"
```

**Expected result**

- 输出 8 行 `PASS`，汇总行 `FR-160 wait-shapes probe: 8 passed, 0 failed`，exit=0。
- 形状 A 断言 `wait` 立即返回（≤1s）**且**守护进程仍存活；形状 B 断言 `wait`
  阻塞 ≥2s **且**进程已被收割——两种形状同时在场，"等到了"与"压根没等"
  在日志里可区分。
- 探针自行回收全部假守护进程（场景 4 交叉验证）。

## 场景 2：25 个站点全部走共享拆解，集合派生且差集为空

**Steps**

```bash
# 旧拼写在库之外必须为空集（库头部注释中的用法示例经剥注释后不计）：
for f in $(git ls-files 'scripts/**/*.sh'); do
  [ "$f" = scripts/lib/gate_daemon.sh ] && continue
  sed -E 's/(^|[[:space:]])#.*$//' "$f" | grep -lE '(kill|wait)( -[A-Za-z0-9]+)? "\$[A-Za-z_]*DAEMON[A-Za-z_]*"' >/dev/null && echo "$f"
done
# 迁移集合（source 了库的门禁，探针除外）必须恰为 25：
for f in $(git ls-files 'scripts/**/*.sh'); do
  grep -qE '^[[:space:]]*\.[[:space:]].*scripts/lib/gate_daemon\.sh' "$f" && echo "$f"
done | grep -v probe-daemon-wait-shapes | wc -l
```

**Expected result**

- 第一段无输出（空集）；第二段输出 `25`。
- 与 FR-160 在 `2e9cb165` 派生的 25 文件清单逐一比对差集为空
  （闭环时实测：EMPTY DIFF）。

## 场景 3：check 16 棘轮及其负夹具

**Steps**

```bash
bash scripts/qa/test-qa-gate-surface.sh 2>&1 | tail -2
bash scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep -E 'fixture 3[345]'
```

**Expected result**

- 验证模式 `17 passed, 0 failed`。
- 夹具 33（裸 kill+wait 重现）经条件 A 失败且诊断具名文件；夹具 34
  （同样的行被注释掉）**通过**——剥注释是承重结构；夹具 35（门禁摘除库
  source）经条件 B 失败且诊断具名缺失的 source 行。

## 场景 4：探针不泄漏（前置条件 3 的行为验证）

**Steps**

```bash
before="$(ps -axo command | grep -cE '/[f]r160-probe|sleep 5' || true)"
bash scripts/qa/probe-daemon-wait-shapes.sh >/dev/null 2>&1
sleep 1
ps -axo pid,command | grep -E 'trap "sleep 2' | grep -v grep
```

**Expected result**

- 最后一条命令无输出：探针启动的三个假守护进程全部被回收，包括形状 A
  故意留活的那个。

## 闭环执行记录（2026-08-10，机器：chenhandeMacBook-Air，全新重装环境）

每个门禁运行前后清点存活进程与临时目录（匹配二进制路径 `/orchestratord`，
不匹配裸词——两次测得调用方命令行携带标记词造成幻影计数 +1/+2），并记录
`df`。全程 25 站点净残留为 **0 进程 / 0 临时目录**；磁盘水位仅因构建产物
与 node_modules 下降，无 QA 泄漏成分。

| 门禁（scripts/qa/）| 修订 | 退出码 | 汇总行 |
|---|---|---|---|
| test-agent-driver-production-parity | e849b5ec | 1 | FR-126 production parity: 10 passed, 1 failed† |
| test-coordination-strangler | e849b5ec | 0 | coordination strangler QA: 20 passed, 0 failed |
| test-pipeline-variable-retirement | e849b5ec | 0 | pipeline variable retirement QA: 13 passed, 0 failed |
| test-agent-driver-abstraction | 2ae4e854 | 0 | FR-116 QA: 8 passed, 0 failed |
| test-process-timeline | 2ae4e854 | 0 | Process timeline QA: 8 passed, 0 failed |
| test-fr013-control-plane-protection | 2ae4e854 | 1 | （首个断言前中止）† |
| test-control-plane-action-audit | 2ae4e854 | 0 | Control-plane action audit QA: 7 passed, 0 failed |
| test-agent-session-control-plane | 2ae4e854 | 0 | Agent session control-plane QA: 6 passed, 0 failed |
| test-session-process-reclamation | 2ae4e854 | 0 | Session process reclamation QA: 13 passed, 0 failed |
| test-attention-inbox | 04e5486d | 0 | Attention Inbox QA: 10 passed, 0 failed |
| test-handoff-safe-resume | 04e5486d | 0 | Handoff and safe resume QA: 8 passed, 0 failed |
| test-process-console-vertical-flow | 04e5486d | 0 | Process Console vertical flow QA: 5 passed, 0 failed |
| test-non-code-workspace | 04e5486d | 0 | FR-117 QA: 7 passed, 0 failed |
| test-expert-resources-governed-editing | 04e5486d | 0 | Expert Resources governed editing QA: 6 passed, 0 failed |
| test-slack-reaction-source | fb889b9b | 0 | Slack reaction source QA: 5 passed, 0 failed |
| test-slack-reaction-task-routing | fb889b9b | 0 | Slack reaction task routing QA: 6 passed, 0 failed |
| test-slack-skill-automation-vertical | fb889b9b | 1 | （disabled fixture 断言失败）† |
| test-source-events-slack | fb889b9b | 0 | Source events and Slack QA: 8 passed, 0 failed |
| test-source-task-binding | fb889b9b | 1 | Source task binding QA: 5 passed, 1 failed† |
| test-source-automation-ui | fb889b9b | 0 | Source automation UI QA passed: 4 gates |
| test-source-task-template | adf2a26e | 0 | Source task template QA: 6 passed, 0 failed |
| test-webhook-trigger | 5390e0f6 | 0 | ALL TESTS PASSED |
| test-per-trigger-webhook-auth | 5390e0f6 | 0 | ALL TESTS PASSED |
| test-coordination-collapse | adf2a26e | 0 | FR-118 QA: 13 passed, 0 failed |
| test-wp05-integration | adf2a26e | 1 | （首个 apply 被 FR-156 强制拒绝，静默截断）† |

† 五个非零退出全部经一次性 worktree 在迁移前 commit 复跑分类为**既有问题**
（与迁移零相关），各有 ticket：`docs/ticket/20260810-*.md`（streaming 驱动穿透
path-shadow、fr013 发现机制腐化、vertical disabled-fixture、binding 五联断言、
wp05 夹具 legacy store_put）。每个失败运行都实际走过新拆解路径：daemon 在
失败时刻存活、被 `gate_daemon_stop` 停止并确认、残留为零——失败路径上的
拆解行为恰是本 FR 的验证对象之一。

分诊政策（已按 FR-160 前置条件 1 执行）：禁止全量旧形状基线跑；单个失败
门禁允许在一次性 worktree 里以迁移前 commit 单跑一次分类，worktree 与其
保留目录随即删除并计入残留清点。

## Checklist

- [ ] 探针 `8 passed, 0 failed`，exit 0，且两种形状的断言各自成对出现
- [ ] 旧拼写派生集合（库之外）为空；source 库的门禁数为 25，与立项清单差集为空
- [ ] `test-qa-gate-surface.sh` 验证模式 17 passed；夹具模式含 33/34/35 全绿
- [ ] 探针运行后无假守护进程存活（场景 4 无输出）
- [ ] 任何门禁运行前后 `/orchestratord` 进程计数与 `$TMPDIR` 目录计数净增长为零
