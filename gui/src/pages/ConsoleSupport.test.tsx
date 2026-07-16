import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ConnectionStatus from "./ConnectionStatus";
import SessionInspector from "./SessionInspector";
import SessionList from "./SessionList";
import System from "./System";
import SourcePanel from "../components/SourcePanel";
import { RoleContext, hasAccess } from "../hooks/useRole";
import type { AgentSession, Role } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../components/SessionPanel", () => ({ default: ({ canControl }: { canControl: boolean }) => <div>session control {String(canControl)}</div> }));
vi.mock("../components/ExpertAgents", () => ({ default: () => <div>agents section</div> }));
vi.mock("../components/ExpertOperations", () => ({ default: () => <div>operations section</div> }));
vi.mock("../components/ExpertResources", () => ({ default: () => <div>resources section</div> }));
vi.mock("../components/ExpertSecret", () => ({ default: () => <div>secrets section</div> }));
vi.mock("../components/ExpertStore", () => ({ default: () => <div>stores section</div> }));
vi.mock("../components/ExpertSystem", () => ({ default: () => <div>runtime section</div> }));
vi.mock("../components/ExpertTrigger", () => ({ default: () => <div>triggers section</div> }));

const active: AgentSession = {
  session_id: "session-active", task_id: "task-1", task_item_id: null, step_id: "implement", agent_id: "coder",
  state: "detached", pid: 42, writer_client_id: null, writer_actor: null, writer_lease_expires_at: null, state_version: 1,
};
const closed: AgentSession = { ...active, session_id: "session-closed", state: "closed", agent_id: "reviewer" };

function roleValue(role: Role) {
  return { role, canAccess: (required: Role) => hasAccess(role, required) };
}

describe("Console supporting pages", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  afterEach(cleanup);

  it("guides connection retry and trims a manual configuration path", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const onRetry = vi.fn();
    render(<ConnectionStatus state={{ kind: "Failed", message: "socket missing" }} onRetry={onRetry} />);
    expect(screen.getByText("socket missing")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "重试连接" }));
    expect(onRetry).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "手动配置" }));
    const connect = screen.getByRole("button", { name: "连接" });
    expect(connect).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText("/path/to/config.yaml"), { target: { value: "  /tmp/control.yaml  " } });
    fireEvent.click(connect);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("connect", { config_path: "/tmp/control.yaml" }));
  });

  it("disables retry and keeps blank manual configuration inert while reconnecting", () => {
    render(<ConnectionStatus state={{ kind: "Reconnecting", attempt: 1, max_attempts: 3 }} onRetry={vi.fn()} />);
    expect(screen.getByRole("button", { name: "连接中..." })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "手动配置" }));
    expect(screen.getByRole("button", { name: "连接" })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("filters global sessions by lifecycle and opens the selected session", async () => {
    vi.mocked(invoke).mockResolvedValue([active, closed]);
    const onSelect = vi.fn();
    render(<SessionList onSelect={onSelect} />);
    expect(await screen.findAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("coder")).toBeVisible();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "closed" } });
    expect(screen.getByText("reviewer")).toBeVisible();
    fireEvent.click(screen.getByRole("listitem"));
    expect(onSelect).toHaveBeenCalledWith("session-closed");
  });

  it("links a session inspector back to its process with role-aware control", async () => {
    vi.mocked(invoke).mockResolvedValue([active]);
    const onBack = vi.fn();
    const onOpenProcess = vi.fn();
    render(<RoleContext.Provider value={roleValue("operator")}>
      <SessionInspector sessionId="session-active" onBack={onBack} onOpenProcess={onOpenProcess} />
    </RoleContext.Provider>);
    expect(screen.getByText("session control true")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "← Sessions" }));
    expect(onBack).toHaveBeenCalledOnce();
    fireEvent.click(await screen.findByRole("button", { name: "Open linked process" }));
    expect(onOpenProcess).toHaveBeenCalledWith("task-1");
  });

  it("renders source bindings, empty state, and read failures", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([{ id: "binding-1", task_id: "task-1", provider: "slack",
      installation_id: "workspace", conversation_id: "channel", thread_id: null, binding_type: "conversation", created_at: "now" }]);
    const first = render(<SourcePanel taskId="task-1" />);
    expect(await screen.findByText("workspace")).toBeVisible();
    expect(screen.getByText("channel / —")).toBeVisible();
    first.unmount();
    vi.mocked(invoke).mockRejectedValueOnce("binding denied");
    render(<SourcePanel taskId="task-2" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("binding denied");
  });

  it("opens the requested System section and switches through visible navigation", () => {
    render(<System initialSection="operations" />);
    expect(screen.getByText("operations section")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Secrets" }));
    expect(screen.getByText("secrets section")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Runtime & Connection" }));
    expect(screen.getByText("runtime section")).toBeVisible();
  });
});
