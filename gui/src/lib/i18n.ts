/**
 * i18n string constants — all user-facing Chinese strings extracted here.
 * Default export is zh (Chinese). Structure supports future multi-language swap
 * via react-intl or similar library without touching component code.
 */

const zh = {
  common: {
    refresh: "刷新",
    cancel: "取消",
    confirm: "确认",
    close: "关闭",
    back: "返回",
    copy: "复制",
    copied: "已复制",
    edit: "编辑",
    save: "保存",
    delete: "删除",
    apply: "应用",
    add: "添加",
    empty: "空",
    loading: "加载中...",
    backToList: "\u2190 返回",
    error: "错误",
  },

  // FR-166: only `mainNav` is read. The nine labels and shortcut hints that used to
  // sit here named a four-item navigation (Attention Inbox / 许愿池 / 进度观察 / 来源)
  // that the console stopped having; App.tsx owns the live five-item nav and its
  // labels. They are removed rather than reworded because a dead string table is a
  // third vocabulary, and the next reader has no way to tell it is not authoritative.
  nav: {
    mainNav: "主导航",
  },

  attention: {
    title: "Attention Inbox",
    subtitle: "只展示需要人决策的异常、审批和阻塞",
    empty: "当前没有需要处理的事项",
    allStates: "活动状态",
    allSeverities: "全部级别",
    mine: "我的事项",
    unassigned: "未分配",
    allAssignees: "全部负责人",
    claim: "认领",
    snooze: "稍后处理",
    resolve: "已处理",
    timeline: "查看任务时间线",
    occurrences: (count: number) => `发生 ${count} 次`,
    readOnly: "当前为只读角色，不能执行变更操作",
    keyboard: "J/K 选择 · C 认领 · R 确认处理 · Enter 查看时间线",
  },

  sources: {
    title: "Sources",
    subtitle: "外部事件、会话关联与路由状态",
    empty: "尚未接收到外部来源事件",
    allStates: "全部路由状态",
    replay: "重新路由",
    openProcess: "打开任务",
    openSlack: "打开 Slack 消息",
    taskBindings: "外部来源关联",
    noBindings: "当前任务没有外部来源关联",
  },

  theme: {
    toggleLight: "切换到浅色模式",
    toggleDark: "切换到深色模式",
  },

  // FR-166: "Wish" was a fourth noun for a thing the rest of the system calls a task.
  // It had no definition anywhere in docs/guide, and its own route already reads
  // `new-task`. These strings now say draft; the `wish-pool` project id they operate
  // on is a wire value and deliberately unchanged.
  wishPool: {
    title: "任务草稿",
    placeholder: "描述你想要实现的功能，比如：我想让用户能通过邮箱注册账号...",
    inputLabel: "需求描述",
    submitLabel: "提交草稿",
    submitting: "提交中...",
    submit: "创建草稿",
    emptyFirst: "还没有草稿，在上方输入你的第一个需求吧",
    emptyFiltered: "没有匹配的草稿",
    wishLabel: (name: string) => `任务草稿: ${name}`,
    filterAll: "全部",
    filterDrafting: "草稿中",
    filterPendingConfirm: "待确认",
    filterConfirmed: "已确认",
    filterCancelled: "已取消",
  },

  wishStatus: {
    drafting: "草稿中",
    pendingConfirm: "待确认",
    paused: "已暂停",
    failed: "失败",
    cancelled: "已取消",
  },

  wishDetail: {
    backToPool: "\u2190 返回任务草稿",
    originalWish: "原始需求",
    noDescription: "(无描述)",
    frDraftPreview: "FR 草稿预览",
    frDraftContent: "FR 草稿内容",
    confirmDev: "确认开发",
    modifyWish: "修改需求",
    cancelWish: "取消",
    cancelTitle: "取消草稿",
    cancelMessage: "确定要取消这个任务草稿吗？此操作不可撤销。",
    cancelConfirm: "确认取消",
    phaseUnderstanding: "正在理解你的需求...",
    phaseDesigning: "正在设计功能方案...",
    phaseWriting: "正在撰写 FR 文档...",
  },

  progressList: {
    title: "进度观察",
    noTasks: "暂无任务",
    realtime: "● 实时",
    startedAt: (time: string) => `开始于 ${time}`,
    failedItems: (count: number) => `${count} 项失败`,
    taskLabel: (name: string) => `任务: ${name}`,
  },

  taskDetail: {
    backLabel: "返回列表",
    pause: "暂停",
    pauseLabel: "暂停任务",
    resume: "恢复",
    resumeLabel: "恢复任务",
    retry: "重试",
    retryLabel: "重试失败项",
    recover: "恢复任务",
    recoverLabel: "恢复任务",
    trace: "跟踪",
    traceLabel: "执行跟踪",
    traceTitle: "执行跟踪",
    expertOn: "专家 \u2713",
    expertOff: "专家",
    expertToggle: "切换专家模式 (Cmd+E)",
    deleteLabel: "删除任务",
    stepProgress: "步骤进度",
    liveLog: "实时日志",
    timeline: "任务时间线",
    timelineHint: "目标、执行、测试证据、失败原因与状态转换",
    timelineEmpty: "尚无可展示的时间线条目。",
    timelineLabel: "任务时间线",
    evidenceLabel: "证据引用",
    loadMore: "加载更多记录",
    viewLabel: "任务详情视图",
    follow: "追踪",
    followLabel: "开始追踪日志",
    stopFollow: "停止",
    stopFollowLabel: "停止追踪日志",
    logWaiting: "等待日志输出...",
    logHint: "点击「追踪」开始接收日志流。",
    logLabel: "任务实时日志",
    deleteTitle: "删除任务",
    deleteMessage: "确定要删除这个任务吗？此操作不可撤销。",
    deleteConfirm: "确认删除",
    searchPlaceholder: "搜索日志...",
    scrollToBottom: "回到底部",
    logLimitHint: (count: number) => `显示最近 ${count} 条`,
  },

  status: {
    running: "运行中",
    completed: "已完成",
    failed: "失败",
    paused: "已暂停",
    pending: "等待中",
    created: "已创建",
    cancelled: "已取消",
  },

  connection: {
    title: "无法连接到 orchestratord",
    possibleCauses: "可能的原因",
    cause1Title: "守护进程未启动",
    cause1Desc: "请在终端执行：",
    cause1Cmd: "orchestratord --foreground",
    cause2Title: "连接地址不正确",
    cause2Desc: "检查 ORCHESTRATOR_SOCKET 环境变量是否指向正确的 socket 文件",
    cause2Env: "ORCHESTRATOR_SOCKET",
    cause3Title: "远程连接证书问题",
    cause3Desc: "检查 ~/.orchestrator/control-plane/ 下的 TLS 证书配置",
    cause3Path: "~/.orchestrator/control-plane/",
    retryConnect: "重试连接",
    connecting: "连接中...",
    manualConfig: "手动配置",
    collapseManual: "收起手动配置",
    manualTitle: "手动配置连接",
    manualDesc: "指定 control-plane 配置文件路径（YAML），用于连接远程 daemon。",
    manualPlaceholder: "/path/to/config.yaml",
    connect: "连接",
  },

  connectionBanner: {
    reconnecting: (attempt: number, max: number) =>
      `连接中断，正在重连... (尝试 ${attempt}/${max})`,
    failed: (msg: string) => `连接失败：${msg}`,
    retry: "重试",
    restored: "已恢复连接",
  },

  expert: {
    navLabel: "专家模式导航",
    workflow: "工作流",
    resources: "资源",
    agents: "Agent",
    store: "Store",
    system: "系统",
    trigger: "触发器",
    secret: "密钥",
    rawData: "原始数据",
  },

  expertWorkflow: {
    noSteps: "暂无工作流步骤数据",
    stepProgress: (finished: number, total: number) => `步骤进度 (${finished}/${total})`,
  },

  expertResources: {
    backToList: "\u2190 返回列表",
    kindFilter: "资源类型",
    catalog: "资源目录",
    loading: "正在加载资源…",
    empty: "此类型暂无资源",
    loadMore: "加载更多",
    open: "打开",
    manifest: "资源 Manifest",
    applying: "正在应用…",
    copyDraft: "复制草稿",
    draftCopied: "草稿已复制",
    manifestCopied: "Manifest 已复制",
    reloadAuthority: "重新加载权威版本",
    authorityReloaded: "权威版本已刷新；你的草稿仍保留，请审查差异后再次应用。",
    confirmTitle: "确认应用资源变更",
    confirmDescription: "Daemon 将重新校验权限、资源版本与完整配置，并写入 Action Audit。",
    confirmApply: "应用已审查变更",
    auditRequest: "审计请求",
  },

  expertAgents: {
    noAgents: "暂无注册的 Agent",
    colName: "名称",
    colStatus: "状态",
    colHealth: "健康",
    colInFlight: "在途任务",
    colActions: "操作",
    drainTitle: "Drain Agent",
    drainMessage: (name: string) =>
      `确定要 drain agent "${name}" 吗？这将停止分配新任务并等待当前任务完成。`,
    drainConfirm: "确认 Drain",
  },

  expertStore: {
    deleteBtnLabel: "删除",
  },

  expertSystem: {
    workerTitle: "Worker 状态",
    active: "活跃",
    idle: "空闲",
    runningTasks: "运行中任务",
    pendingTasks: "待处理任务",
    configuredCount: "配置数",
    lifecycle: "生命周期",
    dbTitle: "数据库状态",
    dbPath: "路径",
    dbVersion: (current: number, target: number) => `${current}/${target}`,
    dbNeedsMigration: "需迁移",
    dbPendingMigrations: (names: string) => `待迁移: ${names}`,
    precheck: "预检查",
    enterMaintenance: "进入维护模式",
    exitMaintenance: "退出维护模式",
    shutdownDaemon: "关闭 Daemon",
    shutdownTitle: "关闭 Daemon",
    shutdownMessage: "确定要优雅关闭 daemon 吗？所有正在运行的任务将被中断。",
    shutdownConfirm: "确认关闭",
  },

  expertTrigger: {
    namePlaceholder: "trigger 名称",
    suspend: "暂停",
    resumeTrigger: "恢复",
    fire: "触发",
    noTriggers: "暂无触发器",
  },

  expertSecret: {
    noKeys: "暂无密钥",
    colKeyId: "Key ID",
    colStatus: "状态",
    colCreatedAt: "创建时间",
    colActions: "操作",
    activeLabel: "活跃",
    revoke: "撤销",
    rotateKey: "轮转密钥",
    revokeTitle: "撤销密钥",
    revokeMessage: (keyId: string) => `确定要撤销密钥 "${keyId}" 吗？此操作不可逆。`,
    revokeConfirm: "确认撤销",
  },

  expertRawData: {
    title: "TaskInfo 原始数据",
  },

  taskList: {
    title: "Tasks",
    refreshBtn: "Refresh",
    noTasks: "No tasks found.",
    colName: "Name",
    colStatus: "Status",
    colProgress: "Progress",
    colUpdated: "Updated",
    colActions: "Actions",
    viewBtn: "View",
  },
} as const;

export default zh;
