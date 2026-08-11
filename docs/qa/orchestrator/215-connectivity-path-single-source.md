---
lifecycle: active
related_fr: FR-163
self_referential_safe: true
---

# QA-215: 路径单源化与陈旧 socket 的连接级探测

FR-163 需求 1、2 的验证。需求 3（就绪信号）与需求 4（连接语义文档）属第二轮治理，
不在本文档范围内。

所有场景均使用独立的 `mktemp` 数据目录与独立的 `HOME`，不触碰开发者自己的 daemon、
数据库或 `~/.orchestratord`（QA §4.7）。

## 背景：本轮修掉的是什么

同一事实在多处派生。治理前实测（`70c85cba`）：data_dir 有 4 处互相独立的派生，
`orchestrator.sock`、`agent_orchestrator.db`、`daemon.pid` 各有 2 处拼写，
分散在互相看不见的 crate 里。其中两处已经真的分叉了：

1. `resolve_data_dir_from_db_path` 把名为 `data` 的父目录读作嵌套布局并返回其祖父目录。
2. `fs_watcher` 的数据目录跳过规则直接读 `ORCHESTRATORD_DATA_DIR`，
   而默认部署下该变量根本没设——跳过是空操作。
3. （治理中由 S4 发现）daemon 把客户端 bundle 写到
   `~/.orchestrator/control-plane/config.yaml`，客户端自动发现却去找
   `~/.orchestratord/control-plane/config.yaml`，差一个字符，该分支从未生效过。

现在唯一的派生点是 `orchestrator_config::paths`。

## S1：`data` 目录名不再被当作布局（单测）

**步骤**

```bash
cargo test -p orchestrator-config --lib paths
cargo test -p orchestrator-security --lib secret_store_crypto::tests
```

**预期**：全部通过。关键断言是往返性质
`data_dir_from_db_path(db_path(d)) == d`，取值包含 `/srv/data`、`/srv/data/data`
这类以 `data` 命名的目录；以及
`the_resolved_data_dir_is_where_the_key_was_seeded`——密钥路径与解析出的
data_dir 必须指向同一处，这正是启发式打破的性质。

**负夹具（已实测）**：把 `data_dir_from_db_path` 改回启发式
（父目录名为 `data` 时返回祖父目录），则
`paths` 的 2 个测试与 `secret_store_crypto` 的 2 个测试同时变红。仅断言
"函数返回了某个目录" 无法发现该缺陷——旧代码正是读写指向不同目录却各自"成功"。

## S2：fs_watcher 的跳过在默认部署下真的生效（单测）

**步骤**

```bash
cargo test -p orchestratord --bin orchestratord fs_watcher
```

**预期**：4 个测试通过。`watch_exclusion` 以参数接收 data_dir，
测试传入的路径不出现在任何环境变量里——这是重点：旧规则读环境变量，
因此对该输入返回 `None`。

**负夹具（已实测，两个方向）**：

| 变异 | 变红的测试 |
| --- | --- |
| 跳过规则改回读 `ORCHESTRATORD_DATA_DIR` | `a_path_inside_the_data_dir_is_excluded_without_consulting_the_environment` |
| `Path::starts_with` 降级为字符串前缀 | `a_sibling_whose_name_merely_begins_with_the_data_dir_is_still_watched` |

两个方向各由不同测试报出，所以日志能说明是往哪边坏的。第二个方向是修复本身
可能引入的过度匹配：修好"漏放"很容易顺手做出"误伤"，而误伤在有人真的建出
`.orchestratord-backup` 之前不产生任何日志。

## S3：客户端 bundle 写入处与自动发现处一致（单测）

**步骤**

```bash
cargo test -p orchestrator-config --lib paths::tests::the_client_bundle_is_written_where_auto_discovery_looks
cargo test -p orchestrator-config --lib paths::tests::the_legacy_discovery_path_is_a_different_place
```

**预期**：通过。第二个断言两处位置确实不同，否则"同时接受新旧两处"这件事本身
就是空的，兼容分支会静默地什么都没测。

## S4：陈旧 socket 的连接级探测（门禁脚本）

**步骤**

```bash
cargo build --release -p orchestratord -p orchestrator-cli
bash scripts/qa/test-stale-socket-discovery.sh
```

**预期**：`FR-163 stale-socket discovery: 7 passed, 0 failed`，退出码 0。

脚本先起一个 UDS daemon 并用 `gate_daemon_kill_hard`（本轮加入
`scripts/lib/gate_daemon.sh` 的受控崩溃停机）杀掉它，使 socket inode 残留，
然后：

- **前提断言**：inode 确实存活。前提不成立是**失败**，不是跳过——否则下面每一条
  都会在一个什么都没修的构建上空过（§4.4 shape 7）。
- **场景 B（无 control-plane 配置）**：诊断必须指名陈旧 socket，且不得复用
  "socket 不存在" 的措辞；并有一条对照断言"socket 真的不存在时仍报旧措辞"，
  否则上一条会因为两种情况说同一句话而通过。
- **场景 A（有 control-plane 配置）**：`task list -o json` 必须**成功**并输出 JSON，
  且输出中不得出现 socket 路径。

**断言强度说明**：全部断言诊断文本与 RPC 结果，**从不断言退出码**。
未修复的构建在场景 B 同样非零退出——以退出码为准的断言会在它本该抓住的缺陷上通过。

**负夹具（已实测）**：把发现步骤 3 改回 `socket.exists()` 并让诊断不再区分两种失败，
门禁报 `4 passed, 3 failed`，三条失败分别指向场景 B 的措辞、场景 A 的 RPC 失败、
以及场景 A 输出里出现了 socket 路径。

**本场景自身的一次修正，记录在此**：场景 A 最初写成"输出里提到 tls / transport
error 即算落到 TLS"，而变异检查发现它在缺陷构建上照样通过——UDS 死路同样打印
`transport error`。该短语什么也没区分。现在的断言是 RPC 真的成功，
这是错误传输上的失败无法伪装的唯一结果。

## S5：布局名字单次拼写的账本门禁（只作为附加条件）

**步骤**

```bash
ruby scripts/qa/connectivity-path-single-source.rb
bash scripts/qa/test-connectivity-path-single-source.sh
```

**预期**：前者报 `7 layout names, each spelled only where permitted`；
后者报 `6 passed, 0 failed`。

门禁的主语是**布局名字的拼写**——data dir、socket、pidfile、db、control-plane 目录、
client 目录、env 变量名，各只允许在具名位置出现。作用范围由 git 派生而非罗列，
并带**镜像条件**：白名单里的位置扫不到东西也算失败，否则门禁可以靠"什么都没看见"变绿。
这是计数类断言，按 §4.4 只能作为**附加**条件——S1~S4 承担行为验证。

**主语选错过一次，记录在此**：最初的主语是"谁从环境读取状态"，结果标出 4 个为了展开
用户路径里的 `~` 而读 `$HOME` 的文件——陈述属实，与 daemon 把 socket 放在哪儿无关。
把匹配器加宽到覆盖你想要的东西，正是它开始覆盖你没想要的东西的方式（§4.4 shape 10）。

**负夹具（`test-connectivity-path-single-source.sh`，已实测）**：注释里出现布局名字
**不得**触发（否则门禁在数散文）；活代码里第二次拼写**必须**触发；把 `paths.rs` 移出
扫描范围必须触发**镜像**条件而非存在条件；清空源码树必须 fail closed。
另有 before/after 两条，使"因无关原因失败"不会被读成"抓住了变异"。

## 已知边界

- 门禁只覆盖 UDS 与 TLS 两条发现分支的落点，不验证 TLS 之后的 RBAC 语义
  （由 QA-58 承担）。
- `discover_socket_path` 的 `ORCHESTRATOR_SOCKET` 分支是具名保留的第二个入口，
  按设计不做连接探测：显式指定传输方式的操作者不应该被静默改道。

## 检查清单

- [ ] S1 `cargo test -p orchestrator-config --lib paths` 与
      `cargo test -p orchestrator-security --lib secret_store_crypto::tests` 全绿
- [ ] S2 `cargo test -p orchestratord --bin orchestratord fs_watcher` 4 项全绿
- [ ] S3 客户端 bundle 写入处与自动发现处一致的两条断言通过
- [ ] S4 `bash scripts/qa/test-stale-socket-discovery.sh` 报 `7 passed, 0 failed`
- [ ] S5 `bash scripts/qa/test-connectivity-path-single-source.sh` 报 `6 passed, 0 failed`
- [ ] 每次改动 `orchestrator_config::paths` 后重跑 S5：新增一个布局名字而不加白名单
      条目应当变红
