import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'Agent Orchestrator',
  description: 'AI-native SDLC automation — declarative workflow and agent orchestration',

  locales: {
    en: {
      label: 'English',
      lang: 'en',
      link: '/en/',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/en/guide/quickstart' },
          { text: 'Why Orchestrator?', link: '/en/why' },
        ],
        sidebar: {
          '/en/guide/': [
            {
              text: 'Guide',
              items: [
                { text: 'Quick Start', link: '/en/guide/quickstart' },
                { text: 'Resource Model', link: '/en/guide/resource-model' },
                { text: 'Workflow Configuration', link: '/en/guide/workflow-configuration' },
                { text: 'CEL Prehooks', link: '/en/guide/cel-prehooks' },
                { text: 'Advanced Features', link: '/en/guide/advanced-features' },
                { text: 'Self-Bootstrap', link: '/en/guide/self-bootstrap' },
                { text: 'CLI Reference', link: '/en/guide/cli-reference' },
              ],
            },
          ],
        },
      },
    },
    zh: {
      label: '中文',
      lang: 'zh-CN',
      link: '/zh/',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh/guide/quickstart' },
          { text: '为什么选择 Orchestrator?', link: '/zh/why' },
        ],
        sidebar: {
          '/zh/guide/': [
            {
              text: '指南',
              items: [
                { text: '快速开始', link: '/zh/guide/quickstart' },
                { text: '资源模型', link: '/zh/guide/resource-model' },
                { text: '工作流配置', link: '/zh/guide/workflow-configuration' },
                { text: 'CEL 前置钩子', link: '/zh/guide/cel-prehooks' },
                { text: '高级特性', link: '/zh/guide/advanced-features' },
                { text: '自举引导', link: '/zh/guide/self-bootstrap' },
                { text: 'CLI 参考', link: '/zh/guide/cli-reference' },
              ],
            },
          ],
        },
      },
    },
  },

  themeConfig: {
    search: {
      provider: 'local',
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/c9r-io/orchestrator' },
    ],
  },
})
