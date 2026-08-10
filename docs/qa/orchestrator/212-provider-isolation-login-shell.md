---
lifecycle: active
related_fr: FR-161
self_referential_safe: true
---

# QA 212: provider 隔离与登录 shell 的解析语义

验证 FR-161：path-shadow 隔离只在非登录 shell 下成立，parity 门禁在
runner 声明的 shell 语义下断言解析，且断言能看见登录 shell 逃逸。

所有场景只读或使用隔离临时目录，不触碰运行中的 orchestratord。

## 场景 1：解析级断言的正反向（行为夹具）

**Steps**

```bash
bash scripts/qa/test-qa-gate-surface.sh --fixture-test 2>&1 | grep -E 'behavioural.*(resolution|entry-level|login-shell)'
```

**Expected result**

三行 PASS：`-c` 下影子胜出、断言通过；毒化 `~/.bash_profile` 前插假 provider
目录后 `-lc` 下断言**失败且诊断具名逃逸解析路径**（root-free 复现 profile
重排——选毒化而非删影子，因为删除是作者预期内的变异）；入口级
`assert_provider_shadow` 在门禁自身 shell 下仍通过（附加条件，不独任）。

## 场景 2：parity 门禁在装有真实 CLI 的机器上全绿

**Steps**

```bash
# 前提：本机 PATH 上存在真实 claude CLI（/opt/homebrew/bin 等 /etc/paths.d 目录）
bash scripts/qa/test-agent-driver-production-parity.sh; echo "exit=$?"
```

**Expected result**

- exit=0，`FR-126 production parity: 11 passed, 0 failed`；
- `streaming-mark-done typed Claude matches recorded ... contract` 为 PASS——
  录制契约只有 fake 能产出，真实 CLI（隔离 HOME 下"Not logged in"退出 1）
  不可能匹配，故该行即"流式步骤命中了 fake"的行为证据；
- 闭环时实测（2026-08-11，本机装有 claude v2.1.220）：全绿。

## 场景 3：声明收窄与双断言在位

**Steps**

```bash
jq -r '.providerIsolationModes."path-shadow"' config/governance/qa-gate-surface.json
grep -n 'assert_provider_shadow\|assert_provider_resolution' scripts/qa/test-agent-driver-production-parity.sh
grep -n 'shell_arg: -c' fixtures/manifests/bundles/agent-driver-production-parity.yaml
```

**Expected result**

- 模式描述包含非登录 shell 条件与两个断言的名字；
- parity 门禁两个断言各恰一次调用，`assert_provider_resolution` 的参数
  `/bin/bash -c` 与夹具 RuntimePolicy 的 `shell_arg: -c` 一致（该耦合由
  DD-175 具名；漂移即重开缺口）。

## Checklist

- [ ] 场景 1 三行为断言 PASS（夹具套件汇总含它们且 0 failed）
- [ ] 场景 2 在装有真实 provider CLI 的 macOS 上 exit 0 且 streaming 行 PASS
- [ ] 场景 3 两处 `-c` 一致、模式描述含收窄条件
- [ ] ubuntu CI 上 parity 门禁保持绿（无 path_helper、无真实 CLI 的对照面）
