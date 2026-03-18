# QA: Public API Doc Comments — Legacy Exemption Cleanup (FR-022)

**Split from**: `docs/qa/orchestrator/75-public-api-doc-comments.md`

## 前置条件

- 工作目录为项目根目录
- Rust 工具链与依赖已安装完成

## 场景 1: 遗留豁免已清理

**步骤**:
```bash
rg -n '#\[allow\(missing_docs\)\]' crates/cli crates/daemon core || true
```

**预期**:
- 命令无结果。
- 说明此前 CLI / daemon 的局部兜底已被移除。

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1: PASS — No legacy missing_docs exemptions found |
