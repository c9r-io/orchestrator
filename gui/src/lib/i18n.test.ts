import { describe, expect, it } from "vitest";
import i18n from "./i18n";

// FR-166 converged the console's vocabulary on Task. Chinese collapses two unrelated
// senses onto 进程 -- the unit of work the console manages, and an operating-system
// process -- so the rename could not be done by substitution, and these assertions
// exist to keep it from being done that way later.
describe("console vocabulary", () => {
  it("names the unit of work a task, not a process", () => {
    expect(i18n.attention.timeline).toBe("查看任务时间线");
    expect(i18n.sources.openProcess).toBe("打开任务");
    expect(i18n.sources.noBindings).toBe("当前任务没有外部来源关联");
    expect(i18n.taskDetail.timeline).toBe("任务时间线");
    expect(i18n.taskDetail.timelineLabel).toBe("任务时间线");
  });

  it("still calls the daemon a process, because it is one", () => {
    // This is the negative fixture for the rename above. A blanket 进程 -> 任务 pass
    // over this file satisfies every assertion in the previous case and turns the
    // daemon-down diagnostic into "任务未启动", which is advice for a different problem.
    expect(i18n.connection.cause1Title).toBe("守护进程未启动");
  });

  it("calls a pending task a draft rather than a wish", () => {
    // "Wish" was never defined in docs/guide and had already leaked into the Chinese
    // console guide undefined. The wire value it operates on is deliberately untouched.
    expect(i18n.wishPool.title).toBe("任务草稿");
    expect(i18n.wishPool.wishLabel("x")).toBe("任务草稿: x");
    expect(i18n.wishDetail.cancelTitle).toBe("取消草稿");
    for (const value of Object.values(i18n.wishPool)) {
      if (typeof value === "string") expect(value).not.toContain("许愿");
    }
  });
});
