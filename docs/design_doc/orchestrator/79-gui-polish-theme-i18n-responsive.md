# Design Doc 079: GUI 体验打磨 — 主题切换 / 动画 / i18n / 响应式 / 构建分发

**FR**: FR-069
**状态**: Closed
**日期**: 2026-03-21

## 概述

本设计文档记录 GUI 产品级体验打磨的 7 项子功能设计决策。

## 1. Light/Dark 主题切换

### 设计决策

- **Hook 模式**：创建 `useTheme()` hook 管理主题状态，返回 `{ theme, toggleTheme }`
- **持久化**：`localStorage("theme")` 存储用户选择
- **系统跟随**：启动时无 localStorage 值则读取 `prefers-color-scheme` 媒体查询
- **DOM 机制**：通过 `document.documentElement.setAttribute("data-theme", "dark")` 激活暗色 tokens
- **UI 入口**：顶栏导航右侧添加月亮/太阳图标按钮

### 关键文件

- `gui/src/hooks/useTheme.ts` — 主题 hook
- `gui/src/App.tsx` — 集成主题切换按钮
- `gui/src/styles/tokens.css` — CSS 变量 `:root` / `[data-theme="dark"]` 已预设

## 2. 等待动画与过渡效果

### 设计决策

- **骨架屏**：创建 `Skeleton` 组件，使用 CSS `skeleton-pulse` 动画替代纯文本 "加载中..."
- **对话框动画**：overlay fade-in (0.2s) + content scale-in (0.2s)
- **状态徽章过渡**：`.status-transition` 类添加 `transition: color 0.3s, background-color 0.3s`
- **连接 banner**：已有 slide-in 动画，保持不变
- **分阶段等待提示**：已在 FR-064 中实现（WishDetail.tsx PROGRESS_PHASES），本次清理重复 keyframe 定义

### 关键文件

- `gui/src/components/Skeleton.tsx` — 骨架屏组件
- `gui/src/styles/tokens.css` — skeleton-pulse、dialog-fade-in、dialog-scale-in 动画

## 3. i18n 预留

### 设计决策

- **常量文件**：`gui/src/lib/i18n.ts` 导出 `zh` 对象，按模块分组（common、nav、wishPool、taskDetail 等）
- **引用方式**：组件通过 `import i18n from "../lib/i18n"` 引用，如 `i18n.common.refresh`
- **函数模板**：动态字符串使用函数，如 `startedAt: (time: string) => \`开始于 ${time}\``
- **不引入第三方库**：纯 TypeScript 常量，未来可接入 react-intl 只需替换导入源

### 覆盖范围

原 18 个含硬编码中文的文件已全部迁移至 `i18n.ts`。`grep` 验证确认 `gui/src/` 下仅 `i18n.ts` 包含中文字符。

## 4. 响应式布局

### 设计决策

- **最小窗口宽度**：`tauri.conf.json` 添加 `"minWidth": 960, "minHeight": 600`
- **CSS 断点**：
  - `@media (max-width: 1000px)` — `.page` 全宽、缩小内边距
  - `@media (min-width: 1200px)` — `.page` 最大宽度扩展至 1200px
- **布局策略**：所有组件已使用 flexbox，在窄屏幕下自然堆叠

## 5. 产品构建与分发

### 设计决策

- **bundle 配置**：`tauri.conf.json` 中 `"targets": "all"` 支持全平台
- **macOS 签名预留**：添加 `"macOS": { "signingIdentity": null }` 占位
- **图标**：保持 `icons/icon.png`（后续需设计师提供 1024×1024 正式图标）
- **前端内嵌**：Vite 构建输出 `gui/dist/`，Tauri build 自动内嵌

## 6. DAG 可视化增强

### 设计决策

- **SVG 渲染**：替换 CSS 顺序列表为 SVG 绘制的 DAG 图
- **并行检测**：相同 `order_no` 的 items 视为并行分支，横向排列
- **布局算法**：按 order_no 分层（layer），每层内节点居中分布
- **视觉元素**：圆角矩形节点 + 箭头边，颜色编码状态（accent=running, success=completed, danger=failed）
- **交互**：可横向滚动查看宽图

### 关键文件

- `gui/src/components/ExpertWorkflow.tsx` — SVG DAG 渲染

## 7. 日志区功能增强

### 设计决策

- **关键字搜索**：输入框实时过滤 + 匹配文本高亮（黄色 `<mark>`）
- **日志条数限制**：保留最近 500 条（`LOG_LIMIT`），超出显示截断提示
- **自动滚动暂停**：监听 scroll 事件，用户向上滚动时暂停自动滚动，显示 "回到底部" 按钮

### 关键文件

- `gui/src/pages/TaskDetail.tsx` — 搜索、限制、暂停逻辑

## 约束遵守

1. 未引入新的 CSS 框架，继续使用 Liquid Glass 设计系统 ✅
2. i18n 仅做字符串抽取，未引入第三方库 ✅
3. 各子项独立实现，互不阻塞 ✅
