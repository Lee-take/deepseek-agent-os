import type {
  AccessMode,
  CapabilityKind,
  Language,
  ModelRoute,
  PolicyDecision,
  RiskLevel,
  ThemeStyle,
  ThinkingLevel,
  WorkspaceScope,
} from "./types";

type TranslationSet = {
  brandTagline: string;
  navLabel: string;
  nav: {
    workbench: string;
    memory: string;
    approvals: string;
  };
  controls: {
    modelRoute: string;
    accessMode: string;
    thinkingLevel: string;
    themeStyle: string;
    language: string;
  };
  modelOptions: Record<ModelRoute, string>;
  accessOptions: Record<AccessMode, string>;
  thinkingOptions: Record<ThinkingLevel, string>;
  scopeOptions: Record<WorkspaceScope, string>;
  themeOptions: Record<ThemeStyle, string>;
  capabilityOptions: Record<CapabilityKind, string>;
  riskOptions: Record<RiskLevel, string>;
  decisionOptions: Record<PolicyDecision, string>;
  workbench: {
    stage: string;
    title: string;
    summary: string;
  };
  package: {
    title: string;
    taskTitle: string;
    taskSummary: string;
    addRecord: string;
    exportPackage: string;
    copyPackage: string;
    importPackage: string;
    packageJson: string;
    importJson: string;
    emptyTitle: string;
    emptyImport: string;
    created: string;
    exported: string;
    copied: string;
    imported: (imported: number, skipped: number) => string;
    noRecords: string;
    copyFailed: string;
    loadFailed: string;
  };
  memory: {
    title: string;
    autoCapture: string;
    noMemories: string;
    loadFailed: string;
    search: string;
    searchPlaceholder: string;
  };
  audit: {
    title: string;
    browser: string;
    emailSend: string;
    computerControl: string;
    empty: string;
    loadFailed: string;
    pending: string;
  };
  inspector: {
    title: string;
    model: string;
    access: string;
    thinking: string;
    scope: string;
    theme: string;
  };
};

export const translations: Record<Language, TranslationSet> = {
  zh: {
    brandTagline: "本地优先 Agent OS",
    navLabel: "主导航",
    nav: {
      workbench: "工作台",
      memory: "记忆",
      approvals: "审批",
    },
    controls: {
      modelRoute: "模型路线",
      accessMode: "访问权限",
      thinkingLevel: "思考强度",
      themeStyle: "界面风格",
      language: "界面语言",
    },
    modelOptions: {
      auto: "DeepSeek 自动",
      flash: "DeepSeek 快速",
      pro: "DeepSeek 专业",
    },
    accessOptions: {
      ask_every_step: "每步询问",
      ask_on_risk: "风险时询问",
      limited_auto: "有限自动",
      full_access: "完全访问",
    },
    thinkingOptions: {
      auto: "自动思考",
      fast: "快速",
      standard: "标准",
      deep: "深入",
    },
    scopeOptions: {
      workspace: "工作区",
    },
    themeOptions: {
      deep: "深色默认",
      ink: "水墨山水",
      porcelain: "青花瓷",
    },
    capabilityOptions: {
      file_read: "读取文件",
      file_write: "写入文件",
      network_search: "联网搜索",
      browser_browse: "浏览网页",
      browser_submit: "提交网页",
      email_read: "读取邮件",
      email_draft: "起草邮件",
      email_send: "发送邮件",
      drive_read: "读取网盘",
      drive_write: "写入网盘",
      terminal_read: "读取终端",
      terminal_write: "写入终端",
      computer_screenshot: "屏幕截图",
      computer_control: "控制电脑",
    },
    riskOptions: {
      low: "低风险",
      medium: "中风险",
      high: "高风险",
      critical: "关键风险",
    },
    decisionOptions: {
      allow: "允许",
      ask: "询问",
      deny: "拒绝",
    },
    workbench: {
      stage: "基础 MVP",
      title: "运营简报工作台",
      summary:
        "第一版已打通桌面工作台、权限控制、DeepSeek 路由默认值与本地内核边界。",
    },
    package: {
      title: "任务记录与工作包",
      taskTitle: "任务标题",
      taskSummary: "任务摘要",
      addRecord: "记录任务",
      exportPackage: "导出工作包",
      copyPackage: "复制",
      importPackage: "导入",
      packageJson: "工作包 JSON",
      importJson: "导入 JSON",
      emptyTitle: "请先填写任务标题。",
      emptyImport: "请先粘贴工作包 JSON。",
      created: "任务记录已写入本地事件库。",
      exported: "工作包已生成。",
      copied: "工作包 JSON 已复制。",
      imported: (imported, skipped) => `导入完成：新增 ${imported} 条，跳过 ${skipped} 条。`,
      noRecords: "暂无任务记录",
      copyFailed: "复制失败，请手动选择 JSON。",
      loadFailed: "任务记录加载失败。",
    },
    memory: {
      title: "自动记忆",
      autoCapture: "由任务记录自动沉淀",
      noMemories: "暂无自动记忆",
      loadFailed: "记忆加载失败。",
      search: "搜索",
      searchPlaceholder: "搜索记忆",
    },
    audit: {
      title: "权限预检",
      browser: "浏览器",
      emailSend: "发邮件",
      computerControl: "控电脑",
      empty: "暂无权限审计",
      loadFailed: "权限审计加载失败。",
      pending: "检查中",
    },
    inspector: {
      title: "运行控制",
      model: "模型",
      access: "权限",
      thinking: "思考",
      scope: "范围",
      theme: "风格",
    },
  },
  en: {
    brandTagline: "Local-first Agent OS",
    navLabel: "Primary",
    nav: {
      workbench: "Workbench",
      memory: "Memory",
      approvals: "Approvals",
    },
    controls: {
      modelRoute: "Model route",
      accessMode: "Access mode",
      thinkingLevel: "Thinking level",
      themeStyle: "Interface style",
      language: "Interface language",
    },
    modelOptions: {
      auto: "DeepSeek Auto",
      flash: "DeepSeek Flash",
      pro: "DeepSeek Pro",
    },
    accessOptions: {
      ask_every_step: "Every step asks",
      ask_on_risk: "Ask on risk",
      limited_auto: "Limited auto",
      full_access: "Full access",
    },
    thinkingOptions: {
      auto: "Thinking auto",
      fast: "Fast",
      standard: "Standard",
      deep: "Deep",
    },
    scopeOptions: {
      workspace: "Workspace",
    },
    themeOptions: {
      deep: "Deep default",
      ink: "Ink landscape",
      porcelain: "Blue porcelain",
    },
    capabilityOptions: {
      file_read: "Read files",
      file_write: "Write files",
      network_search: "Network search",
      browser_browse: "Browse web",
      browser_submit: "Submit web",
      email_read: "Read email",
      email_draft: "Draft email",
      email_send: "Send email",
      drive_read: "Read drive",
      drive_write: "Write drive",
      terminal_read: "Read terminal",
      terminal_write: "Write terminal",
      computer_screenshot: "Screenshot",
      computer_control: "Control computer",
    },
    riskOptions: {
      low: "Low risk",
      medium: "Medium risk",
      high: "High risk",
      critical: "Critical risk",
    },
    decisionOptions: {
      allow: "Allow",
      ask: "Ask",
      deny: "Deny",
    },
    workbench: {
      stage: "Foundation MVP",
      title: "Operations Briefing Workbench",
      summary:
        "The first runnable slice proves the desktop shell, policy controls, DeepSeek routing defaults, and local kernel boundary.",
    },
    package: {
      title: "Task Records and Work Packages",
      taskTitle: "Task title",
      taskSummary: "Task summary",
      addRecord: "Add record",
      exportPackage: "Export package",
      copyPackage: "Copy",
      importPackage: "Import",
      packageJson: "Work package JSON",
      importJson: "Import JSON",
      emptyTitle: "Add a task title first.",
      emptyImport: "Paste work package JSON first.",
      created: "Task record saved to the local event store.",
      exported: "Work package generated.",
      copied: "Work package JSON copied.",
      imported: (imported, skipped) => `Import complete: ${imported} added, ${skipped} skipped.`,
      noRecords: "No task records yet",
      copyFailed: "Copy failed. Select the JSON manually.",
      loadFailed: "Task records failed to load.",
    },
    memory: {
      title: "Auto Memory",
      autoCapture: "Captured from task records",
      noMemories: "No auto memories yet",
      loadFailed: "Memories failed to load.",
      search: "Search",
      searchPlaceholder: "Search memories",
    },
    audit: {
      title: "Permission Check",
      browser: "Browser",
      emailSend: "Email",
      computerControl: "Computer",
      empty: "No permission audits yet",
      loadFailed: "Permission audits failed to load.",
      pending: "Checking",
    },
    inspector: {
      title: "Runtime Controls",
      model: "Model",
      access: "Access",
      thinking: "Thinking",
      scope: "Scope",
      theme: "Style",
    },
  },
};
