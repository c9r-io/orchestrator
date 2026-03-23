# FR-077: Workflow 模板库 — 常见 SDLC 自动化场景预设

## 优先级: P2

## 状态: Proposed

## 背景

用户初次使用 orchestrator 时需要从零编写 workflow manifest，学习成本较高。提供一套开箱即用的 workflow 模板可以让用户快速上手，同时展示平台的核心能力。

## 需求

### 1. 模板库目录结构
```
examples/
├── README.md                        ← 模板索引和使用说明
├── qa-loop/                         ← QA 测试循环
│   ├── manifest.yaml
│   └── README.md
├── pr-review/                       ← PR 代码审查自动化
│   ├── manifest.yaml
│   └── README.md
├── test-fix-loop/                   ← 测试-修复迭代循环
│   ├── manifest.yaml
│   └── README.md
├── deployment-pipeline/             ← 构建-测试-部署流水线
│   ├── manifest.yaml
│   └── README.md
├── security-scan/                   ← 安全扫描 workflow
│   ├── manifest.yaml
│   └── README.md
└── self-evolution/                  ← 自演进循环（高级）
    ├── manifest.yaml
    └── README.md
```

### 2. 每个模板包含
- 完整的 `manifest.yaml`（Workspace + Agent + Workflow 资源）
- `README.md` 说明:
  - 适用场景
  - 前置条件
  - 使用步骤（`orchestrator apply -f manifest.yaml && orchestrator task create`）
  - 自定义指南（哪些字段需要修改）
  - 预期输出

### 3. CLI 模板命令（可选）
- `orchestrator init --template qa-loop` — 从模板初始化项目
- 下载模板到当前目录并自动 apply

### 4. 文档站集成
- 在文档站中新增 "Templates" 章节
- 每个模板配有完整教程

## 验收标准

- [ ] `examples/` 目录包含至少 4 个可工作的模板
- [ ] 每个模板 `orchestrator apply -f` 后能正常创建和运行 task
- [ ] README 说明清晰，新用户可独立完成
- [ ] 文档站 Templates 章节上线
