# Orchestrator Phase 3 Task 02

## Title

统一运行期异常升级表达：让控制面和 trace 对“值得处理的异常”给出更一致的信号

## Goal

在现有 low-output、long-running、transient read 等观测能力之上，建立更一致的异常升级表达，让运行中和事后分析对“异常是否值得处理”有更统一的判断。

## Problem

现在系统已经能看到不少信号，但它们分散在不同出口：

- `task watch` 会显示 `LOW OUTPUT`
- `task trace` 会记录 `low_output_step` / `long_running_step`
- `task logs --follow` 会输出 transient warning

但这些信号之间还缺少更高层次的一致性：

- 同类问题在不同命令中的严重度不完全一致
- 用户需要自行判断哪些异常只是信息、哪些已经值得人工介入
- 事后 trace 与运行期 watch 的信号关联还不够直观

## Scope

- 统一运行期异常的等级和展示语义
- 让 watch / logs / trace 对同类异常更容易关联理解
- 为值得人工介入的运行期异常提供更明确的升级表达

## Out Of Scope

- 不做自动 kill / 自动重试策略
- 不改变任务状态机
- 不引入新的后台守护进程

## Acceptance Criteria

1. 运行中和事后分析对同类异常的命名与严重度更一致。
2. 用户更容易区分“普通信息”和“值得处理的异常”。
3. QA 文档可以对异常场景给出更稳定的预期，而不是依赖观察者主观判断。

## Suggested Verification

- 运行 low-output 与 active-output probe
- 运行长时正常任务与长时异常任务
- 对比 `task watch`、`task logs --follow`、`task trace --json` 中的异常表达
