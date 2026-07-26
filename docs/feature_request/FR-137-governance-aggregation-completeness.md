# FR-137: governance job 聚合清单的完整性断言

## 优先级: P2

## 状态: Proposed

## 背景

FR-134 为解决"job 内串行步骤的首个失败掩盖其后全部诊断"，把 `governance` job 的 20 个门禁步骤
全部改为 `continue-on-error: true`，各自记录真实结果，由末尾的 `Governance result` 步骤汇总后
决定 job 成败。设计是对的——它让一次运行暴露所有问题，而不是只暴露第一个。

但汇总依赖一份**手写枚举**：

```yaml
      - name: Governance result
        if: always()
        env:
          OUTCOMES: |
            liveness=${{ steps.liveness.outcome }}
            surface=${{ steps.surface.outcome }}
            ...共 20 行
```

**没有任何断言守着这份清单的完整性。**

闭环后审计的复现（在 `git archive` 出的副本上）：向 `governance` job 插入一个
`continue-on-error: true` 且 `run: exit 1` 的步骤，不加入 `OUTCOMES`——

```
FR-127 gate surface: 12 passed, 0 failed
```

全绿。该步骤每次运行都失败，而 job 每次都通过。

现实路径不需要恶意。新增一个门禁时，三道既有检查会依次生效：分类检查逼着把脚本登记进
`qa-gate-surface.json`，wiring 检查逼着写出真实的 `run:`，依赖一致性检查逼着补齐命令——
然后**忘了往 `OUTCOMES` 加一行**。三道全绿，门禁静默失效。

这是 FR-134 在别处消灭了六次的那个模式（**枚举式覆盖面只守得住写它时已知的东西**），
出现在它自己为诊断可见性所做的修复里。`DD-145` 的 Known Limits 未记录此项。

**当前是潜伏的**：20 个带 `id:` 的步骤与 20 条 `OUTCOMES` 逐条比对，差集为空。本 FR 是在
它发作之前把它关掉，而不是修复一处已发生的故障。

## 目标

- 让 `governance` job 的聚合覆盖面由解析 workflow 得出，而非由手写清单声明。

## 非目标

- **不**改变 `continue-on-error: true` + 末尾聚合这一结构本身。该结构是 FR-134 需求 8 的正确
  实现，本 FR 只补上它缺失的完整性断言。
- **不**扩展到其他 job。目前只有 `governance` 采用这一模式；若将来别处也采用，检查应随之
  推广，但本 FR 不预先泛化。
- **不**改变任何门禁的分类或 CI 接线。

## 需求

### 1. 聚合完整性检查

- 在 `scripts/qa/test-qa-gate-surface.sh` 中新增一条 check：解析 `governance` job 内全部带
  `id:` 且 `continue-on-error: true` 的步骤，断言每个 `id` 都出现在 `Governance result` 步骤的
  `OUTCOMES` 中。
- 反向亦须断言：`OUTCOMES` 中引用了一个不存在的 step id 即失败——否则重命名步骤会留下一条
  永远解析为空的记录，其效果与遗漏相同。
- 实现可直接复用 `scripts/lib/workflow_model.rb` 的 `steps(path, name)`，无需新解析器。
- 该 check 须进入既有的 `ALL_CHECKS` 注册表，从而自动受 FR-129 建立的两条 meta 断言约束
  （每个 check 都在注册表中、每个注册 check 都至少有一条负向 fixture）。

## 验收标准

- [ ] 新增 check 存在并注册进 `ALL_CHECKS`，两条 meta 断言随之通过
- [ ] 负向 fixture：向 `governance` job 插入一个带 `id:`、`continue-on-error: true` 但不在
      `OUTCOMES` 中的步骤 → 检查失败；补入 `OUTCOMES` 后通过
- [ ] 负向 fixture：`OUTCOMES` 引用一个不存在的 step id → 检查失败
- [ ] fixture 遵循既有隔离约定——只打中目标 check，其余 check 在同一棵树上仍通过
- [ ] 当前仓库状态下新 check 通过（20 个 id 与 20 条 `OUTCOMES` 已一致）
- [ ] `DD-145` 或本 FR 的设计记录写明：聚合清单曾是无人守护的枚举面
- [ ] 全部既有门禁与 CI job 状态不因本 FR 变化

## QA 计划

- **两条负向 fixture 即主证据**：遗漏方向与悬空方向各一条。只做前者会漏掉重命名步骤造成的
  同等失效。
- **隔离断言**：沿用 FR-127 建立的约定——每条 fixture 必须失败于其目标 check，且其余
  check 在同一棵树上仍全部通过，以证明它测的是自己声称测的东西。
- **当前一致性**：以 `comm` 比对 20 个 step id 与 20 条 `OUTCOMES` 的差集为空，作为"本 FR
  修的是潜伏缺陷而非已发作故障"的记录。
- **不需要 CI 实证**：本 FR 不改变任何 job 的运行结果，其证据完全在负向 fixture 内。
