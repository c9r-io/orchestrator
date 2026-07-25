# FR-135: 边界层覆盖率 job 恢复 — bash 3.2 空数组与产物路径

## 优先级: P1

## 状态: Proposed

## 背景

`.github/workflows/ci.yml` 的 `boundary-coverage` job（FR-122 建立的边界层覆盖率非回归门禁）**在最近全部 6 次 CI 运行中均失败**，且失败点在覆盖率生成的第一步，之后的门禁逻辑一次都没有执行过。

真实错误（run `30156418299`，`Generate and enforce boundary coverage` 步骤）：

```
[coverage] collecting instrumented Rust tests
./scripts/coverage-governance.sh: line 38: branch_args[@]: unbound variable
```

`##[error]` 出现在**上传产物**步骤（`No files were found with the provided path: target/coverage-governance/`），那只是继发症状——`if: always()` 让上传步骤在生成失败后仍然运行，于是找不到文件。真正的失败在前一步。

### 根因

`scripts/coverage-governance.sh:25-38`：

```bash
branch_args=()
...
if [[ "$BRANCH_MODE" != "unsupported" ]] && [[ "$rust_channel" == *nightly* ]] && ...; then
  branch_args=(--branch)
fi
...
cargo llvm-cov --workspace --all-targets --all-features \
  "${branch_args[@]}" --json --output-path "$OUTPUT_DIR/rust.json"
```

CI 使用 `dtolnay/rust-toolchain@stable`，所以 `branch_args` 保持为空数组。在 `set -u` 下展开空数组时，**bash 3.2 报 unbound variable，bash 4+ 展开为零个参数**：

```
$ /bin/bash -c 'set -euo pipefail; a=(); printf "%s\n" "${a[@]}"'
/bin/bash: a[@]: unbound variable        # bash 3.2 (macOS 系统自带)

$ /opt/homebrew/bin/bash -c 'set -euo pipefail; a=(); printf "ok\n" "${a[@]}"'
ok                                        # bash 5
```

该 job 是 `runs-on: macos-latest`，workflow 的 `shell: /bin/bash -e` 解析到 macOS 系统自带的 bash 3.2。同一脚本在 ubuntu 上不会触发。

### 为什么长期无人发现

- **姊妹 job 走的是另一条分支**。`coverage-policy-fixtures`（ubuntu + macOS 双平台，均为 success）执行的是 `./scripts/coverage-governance.sh --fixture-test`，而脚本第 15-16 行 `exec node scripts/coverage/test-coverage-governance.mjs` 直接换掉进程，**永远到不了第 38 行**。两个 job 调用同一个脚本，覆盖的却是不相交的代码路径。
- 缺陷随 `1c0b170d`（FR-122 建立本门禁）一同引入，距今 42 个提交。
- 本地 macOS 开发者若用 Homebrew bash（5.x）执行则不复现；用 `/bin/bash` 才复现。

结论与 FR-134 的主题一致：这个门禁被"接线"了，但从未"在守"。

## 目标

- 让 `boundary-coverage` job 在 macOS runner 上真正执行到覆盖率非回归比对，并产出可审计产物。
- 消除 bash 3.2 与 bash 4+ 的空数组语义差异对本仓库全部 CI 脚本的影响。
- 让"上传步骤报错"不再成为掩盖真实失败的表象。

## 非目标

- **不**调整覆盖率阈值或 `coverage/boundary-baseline.json` 的批准基线。恢复 job 后若比对失败，那是需要单独判断的真实回归，不在本 FR 的修复范围内——本 FR 只负责让比对得以发生。
- **不**把 `boundary-coverage` 从 macOS 迁到 ubuntu 来规避问题。该 job 选择 macOS 是为覆盖 Tauri/前端边界，迁移会改变门禁语义。
- **不**引入 nightly Rust 以启用 branch coverage。`BRANCH_MODE` 的 `unsupported` 语义由 FR-122 确定，保持不变。
- **不**处理 `governance` job 与 Slack/strangler job 的失败，那些归 FR-134。

## 需求

### 1. 修复空数组展开

- 将 `"${branch_args[@]}"` 改为 bash 3.2 安全的形式（如 `${branch_args[@]+"${branch_args[@]}"}`），或改用显式条件分支构造命令。
- 全仓扫描同类形态：任何在 `set -u` 下展开可能为空的数组的位置，都需同样处理。`scripts/` 下的脚本均可能运行在 macOS runner 上。

### 2. 建立 shell 兼容性门禁

- 新增检查，断言 `scripts/**/*.sh` 在 bash 3.2 语义下可安全执行到位——至少覆盖"`set -u` 下展开空数组"与 FR-126 期间已遇到的 `mapfile` 不可用两类形态。
- 静态检查即可（如 `shellcheck` 配合 bash 3.2 目标，或针对已知形态的模式扫描），不要求实际在 bash 3.2 下跑通全部脚本。
- 按 FR-127 的分类进入 CI 强制执行面。

### 3. 消除路径不相交导致的假绿

- `coverage-policy-fixtures` 与 `boundary-coverage` 调用同一脚本却覆盖不相交路径，前者全绿掩盖了后者从未执行。需在设计记录中显式写明两者各自验证什么。
- 评估让 `--fixture-test` 之外的主路径至少有一次冒烟执行（可用最小 target 集），使主路径的语法与展开错误在快门禁中即暴露，而非只在重型 job 中。

### 4. 修正失败表象

- `Upload auditable coverage artifacts` 使用 `if: always()` + `if-no-files-found: error`，导致生成失败时报出的是"找不到产物"而非真实原因。调整为在生成步骤失败时不掩盖原始错误（例如去掉 `always()`，或将 `if-no-files-found` 降级为 `warn` 并依赖生成步骤自身的失败）。
- 目标是"CI 摘要第一眼看到的就是根因"，与 FR-134 需求 7 的诊断保真同源。

## 验收标准

- [ ] `boundary-coverage` job 在 macOS runner 上执行到覆盖率非回归比对，不再停在第 38 行
- [ ] 修复前以 `/bin/bash`（3.2）本地运行该脚本可复现 `unbound variable`，修复后不复现
- [ ] shell 兼容性门禁存在并进入 CI；负向 fixture 证明新引入的裸 `"${arr[@]}"`（数组可能为空）会失败
- [ ] 全仓同类形态已清理，或残余点带理由记录
- [ ] 生成步骤失败时，CI 摘要展示的是生成步骤的错误而非上传步骤的"找不到产物"
- [ ] 设计记录写明 `coverage-policy-fixtures` 与 `boundary-coverage` 各自覆盖的路径，以及两者为何不能互相替代
- [ ] job 恢复后若覆盖率比对失败，作为独立议题记录，不在本 FR 内调整基线
- [ ] `cargo test --workspace`、strict Clippy 与其余既有 CI job 状态不因本 FR 变化

## QA 计划

- **bash 3.2 复现与回归**：以 `/bin/bash` 执行修复前后的脚本，断言修复前 `unbound variable`、修复后进入 `cargo llvm-cov`。macOS 系统 bash 即为 3.2，无需容器。
- **兼容性门禁负向 fixture**：向任一脚本插入一处裸 `"${maybe_empty[@]}"`，门禁必须失败；改为安全形式后通过。
- **路径覆盖证明**：证明 `--fixture-test` 与主路径确实不相交（脚本第 15-16 行的 `exec` 即为证据），并在修复后确认主路径至少被执行一次。
- **诊断保真验证**：人为让生成步骤失败，确认 CI 摘要中的首个错误是生成步骤而非上传步骤。
- **CI 实证**：修复后推送并观察真实 workflow 结果。本缺陷只在 macOS runner 上显现，且已被"上传失败"这一表象掩盖了 42 个提交——本地绿与日志表层结论都不足以判定它已修好。
