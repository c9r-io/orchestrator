# FR-161: path-shadow 隔离对登录 shell 下的 provider 解析不成立

## 优先级: P2

## 状态: Proposed

## 背景

所有事实 at `9d290e70`，方法逐条注明；机制在本机（macOS，装有真实 claude-code CLI）实测复现，在 ubuntu CI 上不可复现——这正是问题的一部分。

FR-160 治理清扫中，`test-agent-driver-production-parity.sh` 的 `streaming_typed`
步骤启动了**真实的** claude CLI（v2.1.220，被隔离 HOME 逼成 "Not logged in" 退出 1），
而该门禁声明的隔离模式是 `path-shadow`，且 `assert_provider_shadow` 通过。
原 ticket（`20260810-streaming-driver-bypasses-path-shadow`，随本 FR 立项关闭）
把它记成"流式驱动绕过 PATH 影子解析"，并写下"同一次运行里四个经典 parity
对象都命中了 fake"。**后半句经源码核验不成立，特此更正**：六个经典 agent 的
命令全部是 `echo …`（方法：逐行读 fixture bundle 的 agent 定义），`echo` 是
bash 内建，从不查 PATH——**该门禁里唯一真正解析 `claude` 的就是流式步骤**，
所以 path-shadow 在这个门禁里从未被任何通过的对象行使过。

### 实测机制（单一 spawn 路径，无驱动差异）

流式与经典驱动经同一 `spawn_command_via_shell`（`crates/orchestrator-runner/src/runner/spawn.rs`），
以 `runner.shell = /bin/bash`、`shell_arg = -lc`（默认值，
`crates/orchestrator-config/src/config/runner.rs`）执行。PATH 继承链本身正确
（env allowlist 含 PATH，影子目录在其中）。载荷是 `-lc`：**登录 shell 触发
macOS `/usr/libexec/path_helper`，它把 `/etc/paths` + `/etc/paths.d` 集合排到
最前、把继承的其余条目追加在后**——mktemp 影子目录永远不在 `/etc/paths.d`，
于是被压到 `/opt/homebrew/bin` 之后，真实 CLI 胜出。实测对照（同机）：

- `/bin/bash -c`：影子目录保持第一位，`command -v claude` → 影子。
- `/bin/bash -lc`：影子目录被降位，`command -v claude` → `/opt/homebrew/bin/claude`。

ubuntu 无 `path_helper` 也无真实 CLI，CI 因此恒绿——隔离声明的失效只在
"装有真实 provider CLI 的 macOS 开发机"上可观测，恰是最会烧真实凭据的环境。

### 断言层缺口（§4.4 shape 1）

`assert_provider_shadow`（`scripts/lib/provider_isolation.sh`）确实执行
`command -v` 做解析（治理时核验修正：原文"断言的是 PATH 条目"不准确），
但解析发生在**门禁自己的非登录 shell** 里；runner 用 `-lc` 登录 shell。
两种语义在 macOS 上对同一 PATH 给出不同答案，断言对本缺口因此恒真——
它回答的是"门禁的 shell 会解析到哪"，不是"runner 的 shell 会解析到哪"。

## 需求

### 1. 决定隔离模型对登录 shell 的立场，二选一并成文

- (a) **产品侧**：provider spawn 使用 `-c` 而非 `-lc`（`-c` 已在
  `default_allowed_shell_args`）。波及面：所有依赖登录 profile 环境的步骤命令
  ——需要清点现有 manifest 里依赖 `-lc` 语义的站点（数量未核验）后决策。
- (b) **隔离模型侧**：承认 path-shadow 不覆盖守护进程派生的 provider 命令；
  需要隔离的夹具改用 `spec.driver.binary` 绝对路径钉
  （`build_claude_command` 已支持，`providers.rs`），并把
  `qa-gate-surface.json` 的 `providerIsolationModes.path-shadow` 说明收窄到
  它真正覆盖的面。

### 2. 解析级断言取代条目级断言

`assert_provider_shadow` 增加或改为：以与 runner 相同的 shell 与参数
（`/bin/bash -lc 'command -v <binary>'`）执行一次解析，断言结果落在影子目录。
条目级检查可保留为附加条件，但不得单独构成断言（§4.4：代理只能作为附加条件）。

### 3. parity 门禁在本机环境恢复可跑

无论选 (a) 或 (b)，`test-agent-driver-production-parity.sh` 的 streaming
对象在装有真实 CLI 的 macOS 上必须命中 fake 并回归绿色；QA 176 的已知限度
段落随之收敛或删除。

## 验收标准

- [ ] 需求 1 的选择与理由成文于 DD；若选 (a)，附 `-lc` 依赖站点清点
      （集合由 `git ls-files` + grep 派生，计数带方法）
- [ ] 解析级断言存在，且有负夹具：构造"条目在位但解析不命中"的状态
      （`-lc` + 一个 `/etc/paths.d` 优先目录里的同名假二进制，或等价手段），
      断言必须失败
- [ ] 在装有真实 claude CLI 的 macOS 上运行 parity 门禁：streaming 对象命中
      fake（以留存 stdout 中不出现真实 CLI 的 init 记录为证），门禁全绿
- [ ] ubuntu CI 恒绿不变

## 依赖与关联

- 源自 FR-160 治理清扫（QA 211 记录的五个既有失败之一）；机制定位 2026-08-11。
- 关联 `providerStubs`（CI 的假红后备）与 FR-134（`assert_provider_shadow` 的
  由来）——本 FR 是它的断言对象从"条目"升级为"解析"。
- 原 ticket 的错误前提已在本文档更正；ticket 随立项删除。

## 未核验项（明确标注）

- 依赖 `-lc` 登录语义的现有 manifest 站点数量未清点（需求 1 选 (a) 的前置）。
- 其他 provider CLI（非 claude）在 `/etc/paths.d` 体系下的同型暴露未逐一验证；
  机制推断适用，未实测。
- Linux 桌面发行版是否存在等价的 profile 级 PATH 重排未验证；CI 的 ubuntu
  runner 实测无此行为。
