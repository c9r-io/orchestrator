# QA-119: GUI CLI 功能对齐 — RPC 覆盖补全

**关联设计**: `docs/design_doc/orchestrator/77-gui-cli-rpc-parity.md`
**关联 FR**: FR-067（已闭环）

## 验证场景

## Scenario 0: 入口可见性

**入口**: 进入任务详情页后，点击「专家」按钮展开专家模式面板

| 步骤 | 操作 | 预期结果 |
|------|------|---------|
| 1 | 进入任意任务详情页 | 页面显示任务详情 |
| 2 | 点击「专家」按钮 | 专家模式面板展开 |
| 3 | 确认 Tab 栏显示：系统 / Store / 触发器 / 密钥 | 四个 Expert Tab 均可见 |

**预期**: 专家模式 Tab 栏可见，四类资源 Tab 均可用。

---

### 场景 1: 系统运维 Tab

**步骤**:
1. 专家模式 → 系统 Tab
2. 确认 Worker 状态显示：active/idle/running/pending/configured 计数
3. 确认 DB 状态显示：路径、版本、迁移状态
4. 点击「预检查」→ 确认返回检查结果
5. admin 角色：维护模式切换 + Shutdown 按钮可见
6. read_only 角色：维护模式/Shutdown 按钮不可见

**期望**: 系统信息正确显示，RBAC 控制正确。

---

### 场景 2: Store Tab

**步骤**:
1. 专家模式 → Store Tab
2. 选择 store 类型（env/secret）
3. 确认 key 列表加载
4. 点击 key 查看 value
5. operator+：编辑/删除/添加新 key-value

**期望**: KV 浏览器功能正常。

---

### 场景 3: 触发器 Tab

**步骤**:
1. 专家模式 → 触发器 Tab
2. 确认显示触发器资源 YAML
3. operator+：输入 trigger 名称 → 暂停/恢复/触发

**期望**: 触发器操作正确调用 RPC。

---

### 场景 4: 密钥 Tab

**步骤**:
1. 专家模式 → 密钥 Tab
2. 确认密钥列表显示 key_id + state + created_at
3. 活跃密钥高亮显示
4. admin：轮转按钮 + 撤销按钮（非活跃密钥）

**期望**: 密钥管理功能正确，RBAC 控制正确。

---

### 场景 5: TaskDetail 新按钮

**步骤**:
1. 进入任务详情 → 点击「跟踪」→ 确认 trace JSON 显示
2. failed 任务 → 确认「恢复任务」按钮可见 → 点击调用 TaskRecover

**期望**: 跟踪和恢复功能正常。

---

### 场景 6: 编译验证

**步骤**:
1. `cargo build -p orchestrator-gui` 编译通过
2. 确认 47 个 Tauri 命令全部注册

**期望**: 零编译错误。

---

## Checklist

- [ ] S1: 系统 Tab Worker/DB 状态 + 预检查
- [ ] S2: Store Tab KV 浏览/编辑
- [ ] S3: 触发器 Tab suspend/resume/fire
- [ ] S4: 密钥 Tab 列表/轮转/撤销
- [ ] S5: TaskDetail 跟踪/恢复按钮
- [ ] S6: RBAC 权限控制全部正确
- [ ] S7: RPC 覆盖率 ≥ 80%
