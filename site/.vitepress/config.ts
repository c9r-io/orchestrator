import { defineConfig } from "vitepress";

export default defineConfig({
  title: "Agent Orchestrator",
  description:
    "Harness Engineering control plane for agent-first software delivery",

  locales: {
    en: {
      label: "English",
      lang: "en",
      link: "/en/",
      themeConfig: {
        nav: [
          { text: "Vision", link: "/en/guide/vision" },
          { text: "Guide", link: "/en/guide/quickstart" },
          { text: "Showcases", link: "/en/showcases/hello-world" },
        ],
        sidebar: {
          "/en/guide/": [
            {
              text: "Guide",
              items: [
                { text: "Vision", link: "/en/guide/vision" },
                { text: "Quick Start", link: "/en/guide/quickstart" },
                { text: "Resource Model", link: "/en/guide/resource-model" },
                {
                  text: "Workflow Configuration",
                  link: "/en/guide/workflow-configuration",
                },
                { text: "CEL Prehooks", link: "/en/guide/cel-prehooks" },
                {
                  text: "Advanced Features",
                  link: "/en/guide/advanced-features",
                },
                { text: "Self-Bootstrap", link: "/en/guide/self-bootstrap" },
                { text: "CLI Reference", link: "/en/guide/cli-reference" },
              ],
            },
            {
              text: "Execution Model",
              items: [
                {
                  text: "Agent Driver Model",
                  link: "/en/guide/agent-driver-model",
                },
                {
                  text: "Coordination Tools",
                  link: "/en/guide/coordination-tools",
                },
                {
                  text: "Non-code Workspaces",
                  link: "/en/guide/non-code-workspace",
                },
                {
                  text: "Error Codes",
                  link: "/en/guide/error-codes",
                },
              ],
            },
            {
              text: "Operations",
              items: [
                {
                  text: "Process Console Operations",
                  link: "/en/guide/agent-process-console-v1-operations",
                },
                {
                  text: "Slack Reaction Skill Automation",
                  link: "/en/guide/slack-reaction-skill-automation",
                },
                {
                  text: "Managed Slack Connections",
                  link: "/en/guide/slack-managed-connections",
                },
                {
                  text: "Dedicated Slack App Provisioning",
                  link: "/en/guide/slack-dedicated-app-provisioning",
                },
                {
                  text: "Slack Sandbox Certification Runbook",
                  link: "/en/guide/slack-managed-sandbox-certification-runbook",
                },
              ],
            },
          ],
          "/en/showcases/": [
            {
              text: "Templates",
              items: [
                { text: "Hello World", link: "/en/showcases/hello-world" },
                { text: "QA Loop", link: "/en/showcases/qa-loop" },
                { text: "Plan & Execute", link: "/en/showcases/plan-execute" },
                {
                  text: "Scheduled Scan",
                  link: "/en/showcases/scheduled-scan",
                },
                { text: "FR Watch", link: "/en/showcases/fr-watch" },
                {
                  text: "Webhook Integration",
                  link: "/en/showcases/webhook-integration",
                },
                { text: "Command Rules", link: "/en/showcases/command-rules" },
                {
                  text: "Lightweight Step Run",
                  link: "/en/showcases/lightweight-step-run",
                },
              ],
            },
            {
              text: "Showcases",
              items: [
                {
                  text: "Multi-Model Benchmark",
                  link: "/en/showcases/benchmark-multi-model-execution",
                },
                {
                  text: "Self-Evolution",
                  link: "/en/showcases/self-evolution-execution-template",
                },
                {
                  text: "Self-Bootstrap",
                  link: "/en/showcases/self-bootstrap-execution-template",
                },
                {
                  text: "Full QA Execution",
                  link: "/en/showcases/full-qa-execution",
                },
                {
                  text: "Infinite Evolution Loop",
                  link: "/en/showcases/infinite-evolution-loop",
                },
                {
                  text: "Content Promotion",
                  link: "/en/showcases/promotion-execution",
                },
                {
                  text: "Echo Command Test",
                  link: "/en/showcases/echo-command-test-fixture-execution",
                },
                {
                  text: "Prompt Variable Test",
                  link: "/en/showcases/prompt-variable-parsing-test-fixture-execution",
                },
                {
                  text: "Secret Rotation",
                  link: "/en/showcases/secret-rotation-workflow",
                },
                {
                  text: "Manual Testing",
                  link: "/en/showcases/orchestrator-usage-manual-testing",
                },
                {
                  text: "Typed-Driver Convergence",
                  link: "/en/showcases/streaming-mark-done-convergence",
                },
              ],
            },
          ],
        },
      },
    },
    zh: {
      label: "中文",
      lang: "zh-CN",
      link: "/zh/",
      themeConfig: {
        nav: [
          { text: "愿景", link: "/zh/guide/vision" },
          { text: "指南", link: "/zh/guide/quickstart" },
          { text: "示例", link: "/zh/showcases/hello-world" },
        ],
        sidebar: {
          "/zh/guide/": [
            {
              text: "指南",
              items: [
                { text: "愿景", link: "/zh/guide/vision" },
                { text: "快速开始", link: "/zh/guide/quickstart" },
                { text: "资源模型", link: "/zh/guide/resource-model" },
                {
                  text: "工作流配置",
                  link: "/zh/guide/workflow-configuration",
                },
                { text: "CEL 前置钩子", link: "/zh/guide/cel-prehooks" },
                { text: "高级特性", link: "/zh/guide/advanced-features" },
                { text: "自举引导", link: "/zh/guide/self-bootstrap" },
                { text: "CLI 参考", link: "/zh/guide/cli-reference" },
              ],
            },
            {
              text: "执行模型",
              items: [
                {
                  text: "协作工具",
                  link: "/zh/guide/coordination-tools",
                },
                {
                  text: "非代码工作区",
                  link: "/zh/guide/non-code-workspace",
                },
                {
                  text: "错误码",
                  link: "/zh/guide/error-codes",
                },
              ],
            },
            {
              text: "运维",
              items: [
                {
                  text: "Agent Process Console",
                  link: "/zh/guide/agent-process-console",
                },
              ],
            },
          ],
          "/zh/showcases/": [
            {
              text: "模板",
              items: [
                { text: "Hello World", link: "/zh/showcases/hello-world" },
                { text: "QA Loop", link: "/zh/showcases/qa-loop" },
                { text: "Plan & Execute", link: "/zh/showcases/plan-execute" },
                {
                  text: "Scheduled Scan",
                  link: "/zh/showcases/scheduled-scan",
                },
                { text: "FR Watch", link: "/zh/showcases/fr-watch" },
                {
                  text: "Webhook 集成",
                  link: "/zh/showcases/webhook-integration",
                },
                { text: "Command Rules", link: "/zh/showcases/command-rules" },
                {
                  text: "轻量化单步执行",
                  link: "/zh/showcases/lightweight-step-run",
                },
              ],
            },
            {
              text: "示例",
              items: [
                {
                  text: "多模型 Benchmark",
                  link: "/zh/showcases/benchmark-multi-model-execution",
                },
                {
                  text: "自演化执行",
                  link: "/zh/showcases/self-evolution-execution-template",
                },
                {
                  text: "自举引导执行",
                  link: "/zh/showcases/self-bootstrap-execution-template",
                },
                {
                  text: "全量 QA 执行",
                  link: "/zh/showcases/full-qa-execution",
                },
                {
                  text: "无限演化循环",
                  link: "/zh/showcases/infinite-evolution-loop",
                },
                { text: "内容推广", link: "/zh/showcases/promotion-execution" },
                {
                  text: "Echo 命令测试",
                  link: "/zh/showcases/echo-command-test-fixture-execution",
                },
                {
                  text: "Prompt 变量测试",
                  link: "/zh/showcases/prompt-variable-parsing-test-fixture-execution",
                },
                {
                  text: "密钥轮替",
                  link: "/zh/showcases/secret-rotation-workflow",
                },
                {
                  text: "手动测试指南",
                  link: "/zh/showcases/orchestrator-usage-manual-testing",
                },
                {
                  text: "Typed-Driver 收敛",
                  link: "/zh/showcases/streaming-mark-done-convergence",
                },
              ],
            },
          ],
        },
      },
    },
  },

  themeConfig: {
    search: {
      provider: "local",
    },
    socialLinks: [
      { icon: "github", link: "https://github.com/c9r-io/orchestrator" },
    ],
  },
});
