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

  nav: {
    wishPool: "许愿池",
    progress: "进度观察",
    mainNav: "主导航",
    wishPoolShortcut: "许愿池 (Cmd+1)",
    progressShortcut: "进度观察 (Cmd+2)",
    currentRole: (role: string) => `当前角色: ${role}`,
  },

  theme: {
    toggleLight: "切换到浅色模式",
    toggleDark: "切换到深色模式",
  },

  wishPool: {
    title: "许愿池",
    placeholder: "描述你想要实现的功能，比如：我想让用户能通过邮箱注册账号...",
    inputLabel: "需求描述",
    submitLabel: "提交许愿",
    submitting: "提交中...",
    submit: "许愿",
    emptyFirst: "还没有许过愿，在上方输入你的第一个需求吧",
    emptyFiltered: "没有匹配的许愿",
    wishLabel: (name: string) => `许愿: ${name}`,
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
    backToPool: "\u2190 返回许愿池",
    originalWish: "原始需求",
    noDescription: "(无描述)",
    frDraftPreview: "FR 草稿预览",
    frDraftContent: "FR 草稿内容",
    confirmDev: "确认开发",
    modifyWish: "修改需求",
    cancelWish: "取消",
    cancelTitle: "取消许愿",
    cancelMessage: "确定要取消这个许愿吗？此操作不可撤销。",
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
    describeHint: "使用 resource_describe 查看详情：在上方搜索 \"kind/name\" 格式",
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
