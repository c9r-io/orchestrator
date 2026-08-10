---
lifecycle: active
related_fr: FR-160
---

# DD-174: 共享守护进程拆解 —— `wait` 空操作的终结与它的棘轮

**Status**: Released

记录 FR-160 的设计决定、两条实测出来的机制、以及第一次治理以文件系统事故
告终之后本次治理的执行纪律。验证证据在 [QA 211](../../qa/orchestrator/211-qa-harness-daemon-teardown.md)。

## 库的契约（scripts/lib/gate_daemon.sh）

`gate_daemon_pid_from_file`（pidfile → PID 的唯一被准许入口，缺失/空/非数字
即具名失败）与 `gate_daemon_stop`（SIGTERM → 轮询 ≤10s → 具名升级 SIGKILL →
轮询 ≤5s → 仍存活则具名 return 1；可选第二参数轮询等待守护进程自己的数据目录
pidfile 释放，仅重启站点传入）。超时数值**引自 FR-159/DD-171，未重新推导**。
库不装任何 trap，与 `gate_runlog_arm` 的 trap 链任意次序组合。

两条机制在实现过程中被测出，写进库的头部以防后人"简化"掉：

1. **`kill -0` 对僵尸成功**。`$!` 形状的直接子进程死后在被收割前仍可被信号探测；
   没有 Z 态检查的轮询会对着尸体烧完整个宽限期再"升级"SIGKILL 并报告不可杀。
   故存活谓词是 `gate_daemon_alive`（存在且非 Z 态），且 stop 末尾保留一次
   `wait`，收割子进程情形、对非子进程瞬时无害失败。
2. **信号早于 exec 到达时，收到它的是还携带调用方 EXIT trap 的 fork**。探针
   第一版在 `sh -c ... &` 之后立即 kill，TERM 落在尚未 exec 的 bash 子进程上，
   它临死执行了探针自己的 cleanup——删掉 `$WORK`、顺手停掉了另一形状的
   守护进程。修复是就绪握手：假守护进程装好 trap 后写 ready 文件，探针轮询到
   才开杀。真实门禁天然满足（它们只对已通过端口/CLI 就绪检查的 daemon 发信号）。

## 需求 3 的决定：(a) —— 库只管守护进程

进程组 session 回收留在需要它的两个门禁里（`test-agent-session-control-plane.sh`
的 `reclaim_recorded_sessions`/`reclaim_previous_run_residue`、
`test-session-process-reclamation.sh` 的 `TRACKED_PIDS`）。理由：组回收要求门禁
在 session PID 出现时逐个记录，这是门禁的领域知识，通用拆解逻辑拿不到。

**已知限度，成文**：守护进程已停 ≠ 其子进程已停。三个 ci-required 门禁里
e48fc1b5 的 rm 重试块保留，正是这个残余的第二道防线；`gate_daemon_stop` 确认的
只是守护进程本身退出。

EXIT trap 里调用带 `|| true`（具名行已由库打印，cleanup 不得覆盖判决，也不得
因 daemon 卡死而跳过 session 回收）；脚本中途的重启调用不带，卡死即具名失败。

## 需求 4 的结论：加棘轮，作为 check 16，不加 CI step

shape 2（主）：没有任何东西让其余 24 个站点保持为真，第 26 个门禁会靠抄邻居
诞生。shape 9（次）：FR-159 把正确处置写在一个文件里，记录没有过度声明，但
修复也从未到达其他 24 个。`check_daemon_teardown_shared` 搭在 `test-qa-gate-surface.sh`
的 governance job 便车上：零新增 step，零 `pendingMeasurement` 窗口；验证模式
全量本地实测 4s（含新 check），预算对 (governance + ci-environment-parity)
1793s/2700s 无步进影响。

具名残余（不是掩盖）：check 看不到 `gate_daemon_stop` 运行时是否被执行——
行为半边由探针（每次可重跑）与 QA 211 的 25 行实跑记录承担；变量改名
`SERVER_PID` 可逃逸范围谓词——"DAEMON" 是关于今日树的事实（25/25），放宽
regex 会误伤 FR-160 交叉核验里警告过的 session PID 用法。

check 16 自身的两个草稿踩了它执法的规则，负夹具因此各多一条断言：夹具字符串
在本文件里字面命中条件 A（改为 `%s` 组装）；诊断断言把失败的 check 管道进
`grep -q`，pipefail 下即使命中也报失败（FR-145 的形状，改为先捕获再 grep）。

## 治理事故与本次执行纪律

第一次治理 FR-160 以开发机文件系统故障告终（2026-08-05 重装，成果全丢，机制
不可核验——现场随重装销毁）。本次按 FR 增补的四条前置条件执行，全部留痕于
QA 211：库先行并单独验证；每门禁前后残留清点（净增长即停）；探针成对回收；
`df` 水位记录。每阶段 commit + push——上次事故的直接教训。

清点方法自身修了三轮：`ps | grep` 的测量两次被测量者自己的命令行污染
（管道过滤词 +2、后台 `cargo build -p orchestratord` +2），最终形态锚定二进制
路径段 `/orchestratord( |$)`。测量 harness 会污染测量，这条也适用于清点脚本。

## 本次沿途实测的收获（各有独立 ticket 或修复）

- 五个门禁失败全部经迁移前 commit 复跑分类为既有问题（QA 211 表格 †），
  其中 wp05 是 §4.4 shape 9 落在 FR-156 上：夹具还写着被拒收的 `store_put`。
- `RUST_LOG=warn` 的宿主 shell 让两个 observability 测试变红——测试读宿主而非
  代码（c1fd4dd5 同族），已修为注入式（`resolve_logging_config_with_env`），并
  新增 env-覆盖-CLI 的优先级正向测试。
- 流式 typed 驱动在装有真实 claude-code 的机器上绕过 path-shadow 解析到真实
  CLI——`assert_provider_shadow` 断言的是 PATH 条目，不是驱动实际执行的解析
  （shape 1 对该传输成立），已开 ticket。

## Known limits

- 守护进程已停 ≠ 其子进程已停（决定 (a) 的代价，第二道防线是 rm 重试块）。
- check 16 的运行时可达性与变量改名逃逸，见上文具名残余。
- 释放等待（第二参数）只覆盖"下一次启动复用同一数据目录"的站点集；站点是否
  传它由迁移时逐个判断，无机器强制。
- 25 个站点里有多少守护进程真的派生存活子进程，仍未逐一实测（FR 未核验项
  原样承接）；QA 211 的残留清点给出的是"本轮全部为零"的观测，不是不变量。
- **`scripts/**` 之外仍有旧形状**，具名而非吸收：`docs/qa/script/test-worker-throughput.sh`
  （文档树里的性能辅助脚本）与 QA 58 散文步骤里的八处 kill+wait 片段。两者都在
  本 FR 的派生集合（`git ls-files 'scripts/**/*.sh'`）与 check 16 的扫描范围之外；
  "25 个站点已迁移"的声明不因此过度延伸为"仓库里再无此形状"——shape 9 的
  教训就是这句话的措辞。归属 qa-doc-governance 的常规整备。
