---
lifecycle: active
related_fr: FR-161
---

# DD-175: path-shadow 与登录 shell —— 影子在 `-lc` 下输给 path_helper

**Status**: Released

记录 FR-161 的机制、三个候选方案与用户决策、以及留下的耦合与限度。
验证证据在 [QA 212](../../qa/orchestrator/212-provider-isolation-login-shell.md)。

## 机制（实测，非推断）

流式与经典驱动共用同一 spawn（`spawn_command_via_shell` →
`Command::new(runner.shell).arg(runner.shell_arg)`，默认 `/bin/bash -lc`）。
PATH 继承链本身正确（allowlist 含 PATH，影子目录在其中）；载荷是登录语义：
macOS `/etc/profile` → `/usr/libexec/path_helper` 把 `/etc/paths` +
`/etc/paths.d` 集合重排到最前，继承的其余条目追加在后——mktemp 影子目录
永远不在 `/etc/paths.d`，被压到 `/opt/homebrew/bin`（真实 claude 的家）之后。
同机对照：`-c` 下影子保持第一位，`-lc` 下被降位。

两个曾被记错的前提，原 ticket 与 FR 初稿各中一个，均已在治理中更正：

1. "四个经典对象命中了 fake"不实——它们全是 `echo` 内建，从不查 PATH；
   parity 门禁里唯一解析 `claude` 的就是流式步骤，影子从未被任何通过的
   对象行使过。
2. "`assert_provider_shadow` 只断言 PATH 条目"不准确——它执行 `command -v`
   做真解析，但在**门禁自己的非登录 shell** 里；它回答"门禁的 shell 解析到
   哪"，而缺口在"runner 的 shell 解析到哪"。同一 PATH，两种语义，两个答案。

ubuntu CI 无 path_helper 也无真实 CLI，恒绿——失效只在装有真实 provider
CLI 的 macOS 上可观测，恰是最可能烧真实凭据的环境（本机由 QA_HOME 隔离
兜住了凭据，真实 CLI 以 "Not logged in" 退出）。

## 决策：夹具声明非登录 shell（用户拍板）

三个候选，选 (b')：

- **(a) 产品侧 provider spawn 改 `-c`**：一劳永逸，但改变产品行为——依赖
  登录 profile 重建 PATH 才能解析 provider 二进制的部署（daemon 从无 profile
  上下文启动的未来形态）会断。**已考虑、暂缓**；若将来更多门禁需要，此路
  仍开着，且本 DD 的机制记录即其论证基础。
- **(b) 夹具运行时注入 `binary:` 绝对路径钉**：隔离最硬，但引入夹具模板化
  机制，且 parity 的"夹具与生产 Agent 一致"断言需要为 binary 字段开洞。
- **(b') 夹具 RuntimePolicy 声明 `shell_arg: -c`（选定）**：零产品改动、
  零模板化；`-c` 本就在 `default_allowed_shell_args`。经典对象的录制契约
  不受影响（`echo` 输出与 shell 选择无关）；"夹具与生产一致"断言只看
  driver options，runner 属环境而非契约——此边界成文于此。

配套两件：`providerIsolationModes.path-shadow` 的描述收窄为"仅当被治理
manifest 让 provider 命令在非登录 shell 下运行时成立"，并新增解析级断言。

## 解析级断言（需求 2）

`assert_provider_resolution <shell> <shell_arg> <bindir> [providers...]`
（`scripts/lib/provider_isolation.sh`）：在与 runner 相同的 shell 语义下执行
`command -v`，断言解析落在影子内。入口级 `assert_provider_shadow` 保留为
附加条件（§4.4：代理不得独任）。行为夹具三向：`-c` 通过；毒化
`~/.bash_profile` 前插假 provider 目录后 `-lc` 失败且诊断具名逃逸路径
（root-free 复现 profile 重排，且是"作者最不预期"的变异——删影子才是
预期内情形）；入口级断言同环境仍通过。

## 已知耦合与限度

- **两处 `-c` 必须同步**：夹具 RuntimePolicy 的 `shell_arg: -c` 与 parity
  门禁 `assert_provider_resolution /bin/bash -c` 的参数。漂移即重开缺口。
  断言参数未从 applied 资源反查（那需要门禁在 apply 后查询 RuntimePolicy
  再拼参数，引入的查询面大于它守住的漂移面）；耦合以两处注释互指 + 本条
  具名承载。
- **真实 `/etc/paths.d` 场景未在夹具中复现**（需要 root），由毒化 profile
  等价替代——两者同为"登录流程晚期改写 PATH"，机制同类。
- **其他 provider CLI（codex 等）未逐一实测**；断言与声明按 provider 通用
  书写，机制推断适用。
- path-shadow 模式当前只有 parity 一个门禁（jq 派生）；第二个 path-shadow
  门禁出现时必须同样携带双断言与 `-c` 声明——模式描述已写明，暂无棘轮
  强制（单成员集合上一道棘轮的预算论证不成立，DD-172 的"该不该是门禁"
  之问在此答"不该"；若集合增长再议）。
