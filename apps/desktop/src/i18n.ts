import type {
  AccessMode,
  Language,
  ModelRoute,
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
  workbench: {
    stage: string;
    title: string;
    summary: string;
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
    workbench: {
      stage: "基础 MVP",
      title: "运营简报工作台",
      summary:
        "第一版已打通桌面工作台、权限控制、DeepSeek 路由默认值与本地内核边界。",
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
    workbench: {
      stage: "Foundation MVP",
      title: "Operations Briefing Workbench",
      summary:
        "The first runnable slice proves the desktop shell, policy controls, DeepSeek routing defaults, and local kernel boundary.",
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
