# FR-120: Handoff 恢复审查对话框焦点生命周期

## 优先级: P2

## 状态: Proposed

## 背景

Handoff/Safe Resume 对话框支持手动打开和由 `reviewRequest` 自动打开。手动打开时可以记录触发按钮，但自动打开时 `document.activeElement` 通常是 `body`；现有关闭清理会优先把焦点交还给这个无意义的来源，而不是稳定的 Resume 入口。键盘用户关闭对话框后会丢失操作位置。

这与设计系统“所有交互元素可键盘访问、focus ring 必须可见”的要求不一致。

## 目标

- 为手动、自动、Escape、关闭按钮和成功执行后的所有关闭路径定义一致的焦点策略。
- 保持现有 focus trap、恢复预览安全闸门和权限语义。
- 用组件测试与真实 UI 测试防止焦点生命周期回归。

## 需求

### 1. 打开来源建模

- 只把可连接且可聚焦的元素记录为有效触发来源。
- 自动打开没有有效来源时，使用稳定的 Resume 触发按钮或调用方提供的逻辑目标。
- 路由切换或组件卸载后不得尝试聚焦已断开的节点。

### 2. 对话框内焦点

- 打开后焦点落在可预测的首个安全控件。
- Tab/Shift+Tab 保持在 modal 内循环。
- Escape 与关闭按钮语义一致，busy/不可中断阶段不得产生部分执行。

### 3. 关闭与恢复

- 关闭后焦点返回有效来源；来源不存在时使用可说明的 fallback。
- 不将焦点恢复到 `body`、隐藏元素或 disabled 元素。
- 执行成功触发父级刷新时，不与父组件的路由/焦点管理竞争。

### 4. 可访问性与视觉

- 保持 `role="dialog"`、`aria-modal`、标题关联和可见 focus ring。
- Reduced motion、透明度 fallback 和主题切换不得影响焦点可见性。

## 验收标准

- [ ] 手动打开后通过 Escape/关闭按钮退出，焦点返回原触发按钮
- [ ] 自动打开后退出，焦点返回稳定的 Resume 控件而不是 `body`
- [ ] 来源节点被移除时不会抛错，并选择仍可见、可聚焦的 fallback
- [ ] Tab 与 Shift+Tab 在对话框首尾正确循环
- [ ] 预览失败和执行失败后对话框保持打开且焦点仍位于可操作区域
- [ ] Vitest 和 Playwright 均覆盖手动打开、自动打开、Escape、关闭按钮和节点卸载

## QA 计划

- Vitest/JSDOM：来源校验、focus trap、fallback、失败恢复。
- Playwright：从 Process Detail 手动打开，以及 Attention 一键接力触发的自动打开。
- axe 或等价检查：dialog name、modal、focusable controls 和可见 focus。

## 风险与缓解

- **异步打开竞态**：在 boundary list 返回后再次验证来源与组件存活状态。
- **父级刷新抢焦点**：明确成功关闭与普通取消的所有权顺序。
- **测试环境差异**：关键焦点链路保留 Playwright 浏览器验证，不只依赖 JSDOM。

## 依赖与参考

- `docs/design_doc/orchestrator/107-handoff-and-safe-resume.md`
- `docs/qa/orchestrator/144-handoff-and-safe-resume.md`
- `docs/design-system.md`

