import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ExpertResources from "./ExpertResources";
import { RoleContext, hasAccess } from "../hooks/useRole";
import type { ResourceDescribeResult, ResourceSummary, Role } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const workspace: ResourceSummary = {
  kind: "Workspace",
  name: "default",
  project_id: "default",
  revision: "a".repeat(64),
  source: "resource_store",
};

function described(content = "kind: Workspace\nmetadata:\n  name: default\n"): ResourceDescribeResult {
  return { content, format: "yaml", resource: workspace };
}

function renderAs(role: Role) {
  return render(
    <RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
      <ExpertResources />
    </RoleContext.Provider>,
  );
}

describe("ExpertResources", () => {
  const clipboardWrite = vi.fn().mockResolvedValue(undefined);

  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    clipboardWrite.mockClear();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: clipboardWrite },
    });
  });
  afterEach(cleanup);

  it("renders an authoritative list and lets read-only users open and copy details", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resource_list") return { resources: [workspace], next_cursor: null };
      if (command === "resource_describe") return described();
      throw new Error(`unexpected command ${command}`);
    });
    renderAs("read_only");

    const row = await screen.findByRole("button", { name: "打开 Workspace default" });
    expect(screen.queryByText("kind: Workspace")).not.toBeInTheDocument();
    fireEvent.click(row);

    expect(await screen.findByRole("heading", { name: "Workspace/default" })).toBeVisible();
    expect(screen.getByText(/kind: Workspace/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "编辑" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "复制" }));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledWith(expect.stringContaining("Workspace")));

    fireEvent.click(screen.getByRole("button", { name: "← 返回列表" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "打开 Workspace default" })).toHaveFocus(),
    );
  });

  it("requires reviewed confirmation and reloads authoritative content after apply", async () => {
    let describeCount = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resource_list") return { resources: [workspace], next_cursor: null };
      if (command === "resource_describe") {
        describeCount += 1;
        return describeCount === 1
          ? described()
          : described("kind: Workspace\nmetadata:\n  name: default\nspec:\n  workDir: reviewed\n");
      }
      if (command === "resource_apply") {
        return { message: "updated workspace default", request_id: "req-resource-119" };
      }
      throw new Error(`unexpected command ${command}`);
    });
    renderAs("operator");

    fireEvent.click(await screen.findByRole("button", { name: "打开 Workspace default" }));
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    const editor = screen.getByLabelText("资源 Manifest");
    fireEvent.change(editor, { target: { value: `${(editor as HTMLTextAreaElement).value}\nspec:\n  workDir: reviewed\n` } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));

    const dialog = screen.getByRole("dialog", { name: "确认应用资源变更" });
    expect(within(dialog).getByText(/Workspace\/default · default/)).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "应用已审查变更" })).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Audit reason"), {
      target: { value: "reviewed workspace path change" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "应用已审查变更" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("resource_apply", expect.objectContaining({
      project_id: "default",
      expected_revision: "a".repeat(64),
      require_absent: false,
      reason: "reviewed workspace path change",
    })));
    expect(await screen.findByText(/req-resource-119/)).toBeVisible();
    expect(screen.getByText(/workDir: reviewed/)).toBeVisible();
    expect(screen.queryByLabelText("资源 Manifest")).not.toBeInTheDocument();
  });

  it("preserves the draft when daemon validation rejects apply", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resource_list") return { resources: [workspace], next_cursor: null };
      if (command === "resource_describe") return described();
      if (command === "resource_apply") throw "输入内容不符合要求: invalid workspace path";
      throw new Error(`unexpected command ${command}`);
    });
    renderAs("operator");

    fireEvent.click(await screen.findByRole("button", { name: "打开 Workspace default" }));
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByLabelText("资源 Manifest"), {
      target: { value: "draft-invalid-marker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    const dialog = screen.getByRole("dialog", { name: "确认应用资源变更" });
    fireEvent.change(within(dialog).getByLabelText("Audit reason"), {
      target: { value: "exercise validation" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "应用已审查变更" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("invalid workspace path");
    expect(screen.getByLabelText("资源 Manifest")).toHaveValue("draft-invalid-marker");
  });

  it("reloads a conflicting authority revision without discarding the local draft", async () => {
    let describeCount = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "resource_list") return { resources: [workspace], next_cursor: null };
      if (command === "resource_describe") {
        describeCount += 1;
        if (describeCount === 1) return described();
        return {
          content: "kind: Workspace\nmetadata:\n  name: default\nspec:\n  workDir: authority-v2\n",
          format: "yaml",
          resource: { ...workspace, revision: "b".repeat(64) },
        };
      }
      if (command === "resource_apply") throw "资源已被其他操作更新，请重新加载后再试";
      throw new Error(`unexpected command ${command}`);
    });
    renderAs("admin");

    fireEvent.click(await screen.findByRole("button", { name: "打开 Workspace default" }));
    fireEvent.click(await screen.findByRole("button", { name: "编辑" }));
    fireEvent.change(screen.getByLabelText("资源 Manifest"), {
      target: { value: "local-draft-marker" },
    });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    const dialog = screen.getByRole("dialog", { name: "确认应用资源变更" });
    fireEvent.change(within(dialog).getByLabelText("Audit reason"), {
      target: { value: "exercise conflict" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "应用已审查变更" }));

    fireEvent.click(await screen.findByRole("button", { name: "重新加载权威版本" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("草稿仍保留");
    expect(screen.getByLabelText("资源 Manifest")).toHaveValue("local-draft-marker");
    expect(screen.getByText(/revision bbbbbbbbbbbb/)).toBeVisible();
  });

  it("clears stale rows when switching kinds fails and renders an explicit error", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ resources: [workspace], next_cursor: null })
      .mockRejectedValueOnce(new Error("resource backend unavailable"));
    renderAs("read_only");

    expect(await screen.findByRole("button", { name: "打开 Workspace default" })).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "Workflows" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("resource backend unavailable");
    expect(screen.queryByRole("button", { name: "打开 Workspace default" })).not.toBeInTheDocument();
  });
});
