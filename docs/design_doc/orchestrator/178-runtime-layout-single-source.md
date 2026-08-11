---
lifecycle: active
related_fr: FR-163
---

# DD-178: 运行时布局的单一派生，与陈旧 socket 的连接级探测

**Status**: Released（FR-163 全部四项需求；需求 3、4 见文末"第二轮"）

## 问题

同一事实在多处派生。FR-163 治理前实测（`70c85cba`）：data_dir 有 4 处互相独立的
派生，`orchestrator.sock`、`agent_orchestrator.db`、`daemon.pid` 各有 2 处拼写，
分散在互相看不见的 crate 里——`orchestrator-client` 不依赖 `core`，所以没有任何
一处代码能同时看到两边。

关键不在于"可能会不一致"，而在于**已经有三处真的分叉了，且没有一处让任何测试变红**：

1. `resolve_data_dir_from_db_path` 把名为 `data` 的父目录读作嵌套布局并返回其祖父
   目录。一个把 `ORCHESTRATORD_DATA_DIR` 指向 `$ROOT/data` 的 QA 门禁，密钥被
   boot 播种在 `$ROOT/data/secrets/`，而 SecretStore 的每一次写都去 `$ROOT/secrets/`
   找——写报"no active encryption key"，`secret key list` 却说有 active。
   e5977135 用"显式 env 压倒启发式"打了补丁，机制仍在。
2. `fs_watcher` 的数据目录跳过规则直接读 `ORCHESTRATORD_DATA_DIR`。默认部署下该
   变量根本没设、data_dir 由 home 推出，**跳过是彻底的空操作**：一个 root 落在
   `$HOME` 的 filesystem trigger 会监视 daemon 自己的 `~/.orchestratord`，
   daemon 自己的写入去喂正在监视它们的 trigger。
3. （治理过程中发现）daemon 把本地用户的客户端 bundle 写到
   `~/.orchestrator/control-plane/config.yaml`，而客户端自动发现去找
   `~/.orchestratord/control-plane/config.yaml`——差一个字符。传输发现的第 4 步
   **从未在 daemon 自己的产物上生效过**。

第 3 条的沉积物在仓库里看得见：**10 个以上的 QA 门禁**手工设置
`ORCHESTRATOR_CONTROL_PLANE_CONFIG` 指向写入方的路径来绕过自动发现，
QA-104 甚至把读取方的路径记为"不是前置条件"——文档注意到那个位置从来没被填过，
于是绕着它写。没有人去修，因为每个绕行单独看都是合理的。

另有需求 2 的独立缺陷：传输发现第 3 步只探测 `socket.exists()`，而 socket 比
造它的 daemon 活得久。daemon 在 bind 时清理陈旧 inode，正常关停时也清理
（`lifecycle::cleanup`），但崩溃或 SIGKILL 会留下它。CLI 于是凭一个死 inode 认定
UDS、重试 3 次、报"Is the daemon running?"，而一个能用的 TLS 控制面就在下一步。

## 决策

### `orchestrator_config::paths` 成为唯一派生点

新模块拥有全部布局名字：`data_dir()`、`socket_path`、`pid_path`、`db_path`、
`data_dir_from_db_path`、`control_plane_dir`、`client_control_plane_dir`。

**放在 `orchestrator-config` 而不是 `core`**：客户端 crate 也要解析 socket 路径，
而它不依赖 `core`。`core::config_load::data_dir` 改为 re-export——这正是同一文件里
`now_ts` 已有的安排（从 `orchestrator-persistence` re-export），理由也一样。
代价是 `orchestrator-client` 新增一条依赖边；备选方案（新建 `orchestrator-paths`
叶子 crate）能让已发布的客户端 crate 更瘦，但要多一个 workspace member。

### 启发式退役，而不是继续打补丁

daemon 把数据库开在 data_dir 的直接子级，所以**父目录就是答案，没有什么可推断的**。
`resolve_data_dir_from_db_path` 缩减为 `db_path.parent()`，`data` 目录名分支与
e5977135 加的 env 压制**一并删除**——那个 env 读的存在只是为了压倒一个猜测，
猜测没了它就只能与传进来的路径唱反调。

**没有做 db 迁移，因为不需要**。step 0 核查发现全部 7 处嵌套
`data/agent_orchestrator.db` 写法都在 `#[cfg(test)]` 模块内，生产只写 flat。
所谓"嵌套布局"从来不是代码会写出的布局，而是**调用方把自己的 data_dir 命名为
`data`** 时启发式的误判。FR 原稿把它记为"两种布局同时活着"并要求写迁移说明，
那是 step 0 推翻的四条之一。

### `fs_watcher` 的跳过改为接收参数

`watch_exclusion(abs_path, data_dir)` 从 `state.data_dir` 取值，不再读环境变量。
同一处顺带把字符串前缀匹配换成 `Path::starts_with`（按路径分量比较）——
修"漏放"时很容易顺手做出"误伤"，而 `.orchestratord-backup` 这类兄弟目录被误伤
在有人真的建出它之前不产生任何日志。两个方向各有测试。

### 陈旧 socket：连接探测 + 分化诊断

第 3 步改为 `socket_is_listening`（真的 connect 一次），失败则继续落到第 4 步。
**第 1 步（`ORCHESTRATOR_SOCKET`）刻意不做同样处理**：显式指定传输方式是操作者的
选择，因为它恰好没起来就静默改道去 TLS，等于把操作者自己的配置藏起来。

陈旧判定只在**重试耗尽之后**给出：`connect_uds` 原有的 3 次重试是为了容忍
`exec()` 重启中的 daemon，那段容忍必须保留。两种失败现在措辞不同，因为它们需要
不同的修法：一个是"把 daemon 起起来"，另一个是"上一个 daemon 没干净退出，
这个文件是残骸"。

## 验证

行为断言承担主要证明责任，计数只作附加条件（skill §4.4）。全部负夹具均已实测变红：

| 变异 | 变红处 |
| --- | --- |
| 恢复 `data` 目录名启发式 | `paths` 2 个测试 + `secret_store_crypto` 2 个测试 |
| `fs_watcher` 跳过改回读 env | `a_path_inside_the_data_dir_is_excluded_without_consulting_the_environment` |
| `Path::starts_with` 降级为字符串前缀 | `a_sibling_whose_name_merely_begins_with_the_data_dir_is_still_watched` |
| 第 3 步改回 `exists()` 且诊断不分化 | `test-stale-socket-discovery.sh`：`4 passed, 3 failed` |

三个变异各由**不同**测试报出，所以日志能说明是往哪边坏的。

`scripts/qa/connectivity-path-single-source.rb` 断言每个布局名字在生产代码里只被
拼写一次，作用范围由 `git` 派生而非罗列，并带镜像条件（白名单里的位置扫不到东西
也算失败——否则门禁可以靠"什么都没看见"变绿）。

## 本次治理自己犯的两个错，记录在此

**其一，一个代理断言在它本该抓住的缺陷上通过了。** `test-stale-socket-discovery.sh`
的场景 A 最初写成"输出里提到 tls 或 transport error 即算落到 TLS"。变异检查发现
它在 `exists()` 构建上照样绿——**UDS 死路同样打印 `transport error`**，该短语什么
也没区分。现在断言 RPC 真的成功并返回 JSON，这是错误传输上的失败无法伪装的唯一
结果。这条只有靠"先做变异、再看颜色"才会暴露，光读代码读不出来。

**其二，加固后的匹配器过度伸展。** `connectivity-path-single-source.rb` 最初的主语
是"谁从环境读取状态"，结果标出了 4 个为了展开用户路径里的 `~` 而读 `$HOME` 的文件
——陈述属实，与 daemon 把 socket 放在哪儿毫无关系。把匹配器加宽到覆盖你想要的东西，
正是它开始覆盖你没想要的东西的方式（§4.4 shape 10）。主语因此改为**布局名字的拼写**：
真正重要的是两处代码不能对一个文件名各执一词，而这是可以精确度量的。

**其三，门禁报了绿，而它漏掉一处——由闭环自查（Phase 5）发现，不是由门禁发现。**
改主语之后的规则用的是**精确字面量**（`"control-plane"`），于是看得见
`join("control-plane")`，看不见 `crates/daemon/src/main.rs` 里的
`join("control-plane/uds-policy.yaml")`——同一种重复，只因为多带了一段路径就逃掉了。
那次自查是靠人工 `rg` 一遍布局名字才撞见的，而门禁当时正报"7 个名字各只拼写一次"。

修法与它的反面是同一件事的两半，这正是 shape 10 的完整形态：

- **欠伸展**：规则加宽为允许字面量内带后继路径段，`main.rs` 那处随即被点名，
  代码改为调用 helper。
- **过伸展**：同样的加宽套到 `client dir name` 上，会把 3 处
  `join(".orchestrator/artifacts")` 一并卷进来——那是 workspace root 下的
  **agent 产物目录**，与 `$HOME` 下的客户端 bundle 只是共享前缀的另一个概念。
  因此该项**不**做同样加宽，只匹配裸常量或 `.orchestrator/control-plane`。

两个方向各配一条夹具（复合拼写必须被点名；近似兄弟必须不被点名）。
并且实测记录了**是哪条守卫真正发火**：今天把 `client dir name` naive 加宽，
失败的是**before-run**，因为那 3 处此刻就在树里。近似兄弟那条夹具守的是它们哪天
不在了的那一天——那时 before-run 会重新变绿，只剩这条能发现。两条都留着，
谁报出来是不一样的信息。

## 第二轮：就绪信号与连接语义（需求 3、4）

### 就绪不是存活，`task list` 是代理指标

治理前实测：**24 处轮询散在 23 个门禁**里，全是同样五行的手抄件，
超时预算各执一词——7.5s×1、10s×4、15s×6、20s×12、25s×1，没有一个是推导出来的。
比数字更重的是两点：

1. `task list` 在**套接字接受连接的那一刻**就成功。worker supervisor 虽然在 bind
   前约 560 行就已 spawn（`main.rs:431` vs `main.rs:992`），每个 worker 却是**异步注册**
   的——门禁可以创建出任务，然后眼看着没有人来领。这是套接字探测**看不见**的窗口。
2. `&& break` 在循环耗尽时**静默继续**，门禁于是拿一个没起来的 daemon 跑完整个正文，
   在更下游失败，指认错误的对象。

### 决策：`Health` RPC + `daemon status --wait-ready` + `gate_daemon_wait_ready`

`Health` **聚合既有的** `DbStatus`/`SecretKeyStatus`/`WorkerStatus`，自身不测量任何东西
——关于"迁移是否已到位"的第二种意见，正是第一轮花力气消除的东西。
子系统读不出来时报告为未就绪并把错误放进 detail，**不变成 RPC 失败**：
调用方问的是"你能服务了吗"，用传输错误回答会让"keyring 读不出来"与"daemon 没了"
无法区分——而这恰恰是本端点存在的意义。

`configured_workers == 0` 按定义即就绪：只读 daemon 是操作者的正当选择，
否则那会变成一个永远等不到头的等待。

角色定为 **ReadOnly**，且单测说明了为什么这件事重要而不只是断言它：
角色表的**未映射分支默认 Admin**，漏登记的 RPC 不会报错，它会拿到那个让就绪探针
对所有需要它的调用方都失效的角色。测试因此同时断言 `Health` 与一个不存在的 RPC
**角色不同**——否则"等于 ReadOnly"在默认值恰好是 ReadOnly 的那天也会通过。

helper 的默认超时取它所替代的**五种预算里最宽的那个**，所以没有任何门禁的等待
变短了，这次迁移不可能引入原本不存在的 flake。

`crates/integration-tests` 的服务替身**刻意返回 Unimplemented**：会作答的替身按构造
永远"就绪"，而对着一个不可能不就绪的替身断言就绪契约，等于什么都没断言
（与 FR-164 的审计断言不能写在那里是同一个理由）。

### 24 处收编，两处刻意保留差异

23 处走 `gate_daemon_wait_ready`，1 处直接调用 `daemon status --wait-ready`：
`test-slack-reaction-task-routing.sh` 需要携带传输覆盖，去探测与 TCP daemon
共用数据目录的那个 UDS 实例，覆盖被**带进**了就绪调用而不是丢掉。
`test-failure-visibility.sh` 另外等待磁盘上的密钥文件——就绪报告的是从数据库加载的
keyring，而该门禁后续断言读的是文件，这是**第二个事实**，保留为第二个等待。

### 需求 4：三个主题进用户指南（EN+ZH）

`ORCHESTRATOR_SOCKET` 治理前在 `docs/guide/` 出现 **0 次**；`--bind` 只有一行
"默认：Unix 套接字"，读起来像叠加。现在 EN/ZH 各有：发现顺序 1–5 表格、
`ORCHESTRATOR_SOCKET` 的含义与**第 1 步刻意不做连接探测**的理由
（显式指定传输方式是操作者的选择，因为它恰好没起来就静默改道，等于把配置藏起来）、
`--bind` 与 UDS **互斥**（标志表那一行也已改写）、以及陈旧 socket 的自救。

quickstart 的假陈述已订正：**创建表结构的是第 2 步启动 daemon**，
原第 3 步 `init` 替换为 `daemon status --wait-ready`，并保留一段说明**为什么旧写法是错的**
——`init` 是发往运行中 daemon 的 RPC，daemon 不存在时跑不起来，存在时早已迁移完。
命令本身仍在且无害，它只是从来不是一个安装步骤。

### 第二轮自己踩到的一个坑

抽样验证迁移后的门禁时，用了
`echo "$(basename $g): rc=$?"`——`$(basename …)` 的命令替换会**先执行并重置 `$?`**，
于是读到的永远是 0，四个门禁全部"通过"。这正是 skill §4.6.4"直接捕获退出码"
所防的东西，而它当时正作用在认证者自己身上。同一次误读还掩盖了另一件事：
门禁用的是 `target/debug` 而我只重建了 `target/release`，真实状态是"命令行旗标不存在"。
**退出码必须在命令之后立即捕获到变量里**，不能放进任何命令替换的同一行。

## 已知边界

- `discover_socket_path` 的 `ORCHESTRATOR_SOCKET` 分支是**具名保留**的第二个入口。
  它消费一个 `data_dir()` 不认识的变量，按设计不做连接探测。终局是 4 → 2，不是 4 → 1。
- `orchestrator-runner/src/runner/policy.rs` 具名保留 `daemon.pid` 的第二处拼写：
  那不是路径构造，而是筛查 agent 命令里 `kill $(cat .../daemon.pid)` 形状的子串守卫，
  它按名字匹配正是因为名字就是待筛查命令里出现的东西。
- `rust_source.rb` 的 `test*.rs` basename 排除有已知缺陷（open ticket
  `20260811-rust-source-test-basename-hides-production.md`）。**已核验并每次重新核验**：
  门禁最后一项检查会在排除被解除后重扫，若有拼写被藏住即失败。首次运行时它确实
  报出了 `core/src/test_utils.rs`——该文件已改为调用 helper，而不是被豁免。
- 门禁只覆盖 UDS 与 TLS 两条发现分支的落点，不验证 TLS 之后的 RBAC 语义（QA-58 承担）。
- `Health` 的 RBAC 层级由角色表单测钉住，不由门禁验证：门禁级检查需要 control-plane
  材料，绕更远的路证明同一件事。未映射分支默认 Admin 是这条的要害。
- 就绪只覆盖 migrations / keyring / workers 三项。Slack gateway、fs_watcher、
  trigger engine 未入列——它们不是"任务能否被领走"的前置条件，把它们加进来会让
  就绪等待被无关子系统卡住。新增子系统时，`serving` 由服务端聚合，调用方不必改。
