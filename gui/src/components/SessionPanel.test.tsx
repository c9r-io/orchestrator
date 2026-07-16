import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import SessionPanel from "./SessionPanel";
import type { AgentSession } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const listeners = new Map<string, (event: { payload: unknown }) => void>();

const session: AgentSession = {
  session_id: "session-1", task_id: "task-1", task_item_id: null, step_id: "implement",
  agent_id: "coder", state: "detached", pid: 42, writer_client_id: null, writer_actor: null,
  writer_lease_expires_at: null, state_version: 3,
};

describe("SessionPanel", () => {
  beforeEach(() => {
    listeners.clear();
    vi.spyOn(crypto, "randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000001");
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      listeners.set(String(name), handler as (event: { payload: unknown }) => void);
      return () => undefined;
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "agent_session_list") return [session];
      if (command === "agent_session_attach") return { fencing_token: 7, lease_expires_at: "2026-07-17T01:00:00Z" };
      if (command === "agent_session_heartbeat") return "2026-07-17T01:01:00Z";
      return null;
    });
  });
  afterEach(cleanup);

  it("follows transcript offsets without duplicates and reconnects from the committed byte", async () => {
    const { unmount } = render(<SessionPanel taskId="task-1" canControl={false} />);
    expect(await screen.findByRole("region", { name: "Agent sessions" })).toBeVisible();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_agent_session_read", { session_id: "session-1", offset: 0 }));
    act(() => listeners.get("agent-session-output-session-1")?.({
      payload: { offset: 0, next_offset: 5, text: "hello", eof: false, redacted: false },
    }));
    act(() => listeners.get("agent-session-output-session-1")?.({
      payload: { offset: 0, next_offset: 5, text: "duplicate", eof: false, redacted: false },
    }));
    expect(screen.getByRole("log")).toHaveTextContent("hello");
    expect(screen.getByRole("log")).not.toHaveTextContent("duplicate");
    expect(screen.getByText(/Reader: attached/)).toBeVisible();
    unmount();
    expect(invoke).toHaveBeenCalledWith("stop_agent_session_read", { session_id: "session-1" });
  });

  it("acquires one writer, sends idempotent input, releases it, and closes by state version", async () => {
    render(<SessionPanel sessionId="session-1" canControl />);
    fireEvent.click(await screen.findByRole("button", { name: "Request control" }));
    const input = await screen.findByLabelText("Session input");
    fireEvent.change(input, { target: { value: "continue" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_session_send_input", expect.objectContaining({
      session_id: "session-1", fencing_token: 7, text: "continue",
      idempotency_key: "00000000-0000-4000-8000-000000000001",
    })));
    expect(input).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Release control" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Request control" })).toBeVisible());
    fireEvent.click(screen.getByRole("button", { name: "Close session" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_session_close", {
      session_id: "session-1", state_version: 3, reason: "Closed from task detail",
      idempotency_key: "00000000-0000-4000-8000-000000000001",
    }));
  });

  it("shows read-only reasoning and stream errors without mutation controls", async () => {
    render(<SessionPanel canControl={false} />);
    expect(await screen.findByText(/Read-only access/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Request control" })).not.toBeInTheDocument();
    act(() => listeners.get("stream-error-agent-session-session-1")?.({ payload: "reader disconnected" }));
    expect(screen.getByText("reader disconnected")).toBeVisible();
    expect(screen.getByText("Disconnected")).toBeVisible();
  });

  it("renders nothing when no session exists and surfaces list failures", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("session list denied"));
    const { container } = render(<SessionPanel canControl={false} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_session_list", { task_id: null }));
    expect(container).toBeEmptyDOMElement();
  });
});
