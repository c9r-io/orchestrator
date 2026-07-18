import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ExpertPanel from "./ExpertPanel";
import { RoleContext, hasAccess } from "../hooks/useRole";
import type { Role, TaskDetail } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const detail: TaskDetail = {
  id: "task-1",
  name: "Automation rollout",
  status: "running",
  goal: "Ship the workflow",
  total_items: 4,
  finished_items: 1,
  failed_items: 1,
  created_at: "2026-07-18T00:00:00Z",
  updated_at: "2026-07-18T00:01:00Z",
  project_id: "project-1",
  workflow_id: "qa-loop",
  items: [
    { id: "one", qa_file_path: "docs/qa/complete.md", status: "completed", order_no: 1 },
    { id: "two", qa_file_path: "docs/qa/running.md", status: "running", order_no: 2 },
    { id: "three", qa_file_path: "", status: "failed", order_no: 2 },
    { id: "four", qa_file_path: "docs/qa/pending.md", status: "pending", order_no: 3 },
  ],
};

function renderAs(role: Role) {
  return render(
    <RoleContext.Provider
      value={{ role, canAccess: (required) => hasAccess(role, required) }}
    >
      <ExpertPanel taskDetail={detail} />
    </RoleContext.Provider>,
  );
}

describe("ExpertPanel", () => {
  const clipboardWrite = vi.fn();

  beforeEach(() => {
    clipboardWrite.mockReset();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: clipboardWrite.mockResolvedValue(undefined) },
    });
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      switch (command) {
        case "resource_get":
          return { content: `kind: ${(args as { resource?: string })?.resource}`, format: "yaml" };
        case "agent_list":
          return [
            { name: "coder", enabled: true, lifecycle_state: "active", in_flight_items: 1, capabilities: ["code"], is_healthy: true },
            { name: "reviewer", enabled: true, lifecycle_state: "cordoned", in_flight_items: 0, capabilities: ["review"], is_healthy: false },
          ];
        case "agent_cordon":
        case "agent_uncordon":
        case "agent_drain":
          return `${command} complete`;
        case "store_list":
          return [{ key: "REGION", value_json: "\"jp\"", updated_at: "now" }];
        case "store_get":
          return "\"jp\"";
        case "store_put":
        case "store_delete":
          return `${command} complete`;
        case "worker_status":
          return { pending_tasks: 2, active_workers: 1, idle_workers: 1, running_tasks: 1, configured_workers: 2, lifecycle_state: "running", shutdown_requested: false };
        case "db_status":
          return { db_path: "/tmp/orchestrator.db", current_version: 31, target_version: 32, is_current: false, pending_names: ["source_connections"] };
        case "check":
          return { content: "all checks passed" };
        case "maintenance_mode":
          return { message: "maintenance changed" };
        case "shutdown":
          return "shutdown requested";
        case "trigger_suspend":
        case "trigger_resume":
          return `${command} complete`;
        case "trigger_fire":
          return { task_id: "task-triggered", message: "triggered" };
        case "secret_key_status":
          return {
            active_key: { key_id: "active-key-123456", status: "active", created_at: "today" },
            all_keys: [
              { key_id: "active-key-123456", status: "active", created_at: "today" },
              { key_id: "retiring-key-987654", status: "retiring", created_at: "yesterday" },
            ],
          };
        case "secret_key_rotate":
          return { message: "rotated", resources_updated: 2 };
        case "secret_key_revoke":
          return "revoked";
        default:
          throw new Error(`unexpected command: ${command}`);
      }
    });
  });

  afterEach(cleanup);

  it("visualizes parallel workflow states and copies bounded raw task data", async () => {
    renderAs("read_only");
    expect(screen.getByText("步骤进度 (1/4)")).toBeVisible();
    expect(document.querySelectorAll("svg g")).toHaveLength(4);
    expect(document.querySelectorAll("svg line")).toHaveLength(4);

    fireEvent.click(screen.getByRole("button", { name: "原始数据" }));
    expect(screen.getByText(/"id": "task-1"/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "复制" }));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledWith(expect.stringContaining("Automation rollout")));
    expect(screen.getByRole("button", { name: "已复制" })).toBeVisible();
  });

  it("operates resources, agents, and stores through role-gated controls", async () => {
    renderAs("admin");

    fireEvent.click(screen.getByRole("button", { name: "资源" }));
    fireEvent.click(screen.getByRole("button", { name: "workspaces" }));
    expect(await screen.findByText("kind: workspaces")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Agent" }));
    expect(await screen.findByText("coder")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Cordon" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_cordon", { agentName: "coder" }));
    fireEvent.click(screen.getByRole("button", { name: "Uncordon" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_uncordon", { agentName: "reviewer" }));
    fireEvent.click(screen.getAllByRole("button", { name: "Drain" })[0]);
    fireEvent.click(within(screen.getByRole("dialog", { name: "Drain Agent" })).getByRole("button", { name: "确认 Drain" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_drain", { agentName: "coder" }));

    fireEvent.click(screen.getByRole("button", { name: "Store" }));
    fireEvent.click(await screen.findByText("REGION"));
    expect(await screen.findByText("\"jp\"")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "\"us\"" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("store_put", {
      store: "env",
      key: "REGION",
      valueJson: "\"us\"",
    }));
    fireEvent.change(screen.getByPlaceholderText("key"), { target: { value: "TEAM" } });
    fireEvent.change(screen.getByPlaceholderText("value (JSON)"), { target: { value: "\"core\"" } });
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("store_put", {
      store: "env",
      key: "TEAM",
      valueJson: "\"core\"",
    }));
  });

  it("runs system, trigger, and secret administration with reviewed destructive actions", async () => {
    renderAs("admin");

    fireEvent.click(screen.getByRole("button", { name: "系统" }));
    expect(await screen.findByText("Worker 状态")).toBeVisible();
    expect(screen.getByText(/待迁移: source_connections/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "预检查" }));
    expect(await screen.findByText("all checks passed")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "进入维护模式" }));
    fireEvent.click(screen.getByRole("button", { name: "退出维护模式" }));
    fireEvent.click(screen.getByRole("button", { name: "关闭 Daemon" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "关闭 Daemon" })).getByRole("button", { name: "确认关闭" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("shutdown", { graceful: true }));

    fireEvent.click(screen.getByRole("button", { name: "触发器" }));
    const triggerName = await screen.findByPlaceholderText("trigger 名称");
    fireEvent.change(triggerName, { target: { value: " nightly " } });
    fireEvent.click(screen.getByRole("button", { name: "暂停" }));
    fireEvent.click(screen.getByRole("button", { name: "恢复" }));
    fireEvent.click(screen.getByRole("button", { name: "触发" }));
    await waitFor(() => expect(screen.getByText("triggered (task: task-triggered)")).toBeVisible());
    expect(invoke).toHaveBeenCalledWith("trigger_suspend", { triggerName: "nightly" });
    expect(invoke).toHaveBeenCalledWith("trigger_resume", { triggerName: "nightly" });

    fireEvent.click(screen.getByRole("button", { name: "密钥" }));
    expect(await screen.findByText("active-key-1")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "轮转密钥" }));
    await waitFor(() => expect(screen.getByText("rotated (2 resources updated)")).toBeVisible());
    fireEvent.click(screen.getByRole("button", { name: "撤销" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "撤销密钥" })).getByRole("button", { name: "确认撤销" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("secret_key_revoke", {
      keyId: "retiring-key-987654",
      force: true,
    }));
  });
});
