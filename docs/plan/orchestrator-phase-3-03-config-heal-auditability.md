# Orchestrator Phase 3 Task 03

## Title

提升 active config 自愈的可审计性与可解释性

## Goal

在已有自愈能力基础上，让“何时发生过自愈、修了什么、为什么修”更容易被长期追踪和审计，而不只停留在当前进程的临时 notice。

## Problem

Phase 2 已经让 active config 能自动修复一类历史漂移，但当前可见性仍偏短期：

- `config_auto_healed` 只在当前进程的 `check` 中可见
- 进程重启后，临时 notice 会消失
- 虽然数据库版本里有 `author=self-heal`，但缺少更直接的历史解释

这意味着：

- 系统已经自愈了，但后续调查时仍要反查版本表和代码逻辑
- 用户很难快速知道“到底修过什么”

## Scope

- 增强自愈结果的历史可读性与审计信息
- 让用户能更直接追踪最近一次或历史上的 self-heal 记录
- 在不破坏现有 schema 的前提下提升可解释性

## Out Of Scope

- 不做全新的配置版本管理系统
- 不自动回放或对比仓库 manifest
- 不扩大自愈白名单范围到高风险修复

## Acceptance Criteria

1. 自愈事件在进程外也能更容易追踪，而不是只存在内存 notice。
2. 用户能更直接看到最近一次自愈修复了什么。
3. 自愈历史比当前仅靠 `author=self-heal` 更容易理解。

## Suggested Verification

- 触发一次可修复的历史漂移
- 重启新进程后验证仍能定位最近一次 self-heal
- 验证输出能包含变更摘要，而不是只看到 version/author
