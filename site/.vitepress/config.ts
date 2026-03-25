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
          { text: 'Showcases', link: '/en/showcases/benchmark-multi-model-execution' },
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
          '/en/showcases/': [
            {
              text: 'Showcases',
              items: [
                { text: 'Multi-Model Benchmark', link: '/en/showcases/benchmark-multi-model-execution' },
                { text: 'Self-Evolution', link: '/en/showcases/self-evolution-execution-template' },
                { text: 'Self-Bootstrap', link: '/en/showcases/self-bootstrap-execution-template' },
                { text: 'Full QA Execution', link: '/en/showcases/full-qa-execution' },
                { text: 'Infinite Evolution Loop', link: '/en/showcases/infinite-evolution-loop' },
                { text: 'Content Promotion', link: '/en/showcases/promotion-execution' },
                { text: 'Echo Command Test', link: '/en/showcases/echo-command-test-fixture-execution' },
                { text: 'Prompt Variable Test', link: '/en/showcases/prompt-variable-parsing-test-fixture-execution' },
                { text: 'Manual Testing', link: '/en/showcases/orchestrator-usage-manual-testing' },
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
          { text: '示例', link: '/zh/showcases/benchmark-multi-model-execution' },
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
          '/zh/showcases/': [
            {
              text: '示例',
              items: [
                { text: '多模型 Benchmark', link: '/zh/showcases/benchmark-multi-model-execution' },
                { text: '自演化执行', link: '/zh/showcases/self-evolution-execution-template' },
                { text: '自举引导执行', link: '/zh/showcases/self-bootstrap-execution-template' },
                { text: '全量 QA 执行', link: '/zh/showcases/full-qa-execution' },
                { text: '无限演化循环', link: '/zh/showcases/infinite-evolution-loop' },
                { text: '内容推广', link: '/zh/showcases/promotion-execution' },
                { text: 'Echo 命令测试', link: '/zh/showcases/echo-command-test-fixture-execution' },
                { text: 'Prompt 变量测试', link: '/zh/showcases/prompt-variable-parsing-test-fixture-execution' },
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
