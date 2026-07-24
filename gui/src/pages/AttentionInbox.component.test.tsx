import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AttentionInbox from "./AttentionInbox";
import { RoleContext, hasAccess } from "../hooks/useRole";
import type { AttentionItem, Role } from "../lib/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("../lib/telemetry", () => ({ recordUiMetric: vi.fn() }));

const listeners = new Map<string, (event: { payload: unknown }) => void>();

function attention(overrides: Partial<AttentionItem> = {}): AttentionItem {
  return {
    id: "attention-1", project_id: "project-1", task_id: "task-1", task_item_id: null,
    step_id: "test", session_id: null, kind: "step_failed", severity: "intervention", state: "open",
    title: "Approval required", summary: "Choose a recovery", requested_decision_json: "{\"question\":\"Retry now?\"}",
    actions: [{ id: "escalate", label: "Escalate", required_role: "operator", confirmation: "required", input_schema_json: "{}" }],
    assignee: null, occurrence_count: 1, reopen_count: 0, version: 1,
    created_at: "2026-07-17T00:00:00Z", updated_at: "2026-07-17T00:00:00Z",
    last_occurred_at: new Date().toISOString(), snoozed_until: null, resolved_at: null,
    ...overrides,
  };
}

function renderAs(
  role: Role,
  onOpenTask = vi.fn(),
  nativeNotificationsEnabled = true,
  onOpenSourceRoute?: (routeId: string) => void,
) {
  return {
    onOpenTask,
    ...render(<RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
      <AttentionInbox
        nativeNotificationsEnabled={nativeNotificationsEnabled}
        onOpenTask={onOpenTask}
        onOpenSourceRoute={onOpenSourceRoute}
      />
    </RoleContext.Provider>),
  };
}

describe("AttentionInbox component", () => {
  let current: AttentionItem;

  beforeEach(() => {
    current = attention();
    listeners.clear();
    vi.spyOn(crypto, "randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000002");
    vi.mocked(listen).mockImplementation(async (name, handler) => {
      listeners.set(String(name), handler as (event: { payload: unknown }) => void);
      return () => undefined;
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") return { items: current.state === "resolved" ? [] : [current], latest_change_id: 10 };
      if (["attention_claim", "attention_snooze", "attention_resolve", "attention_execute_action"].includes(command)) {
        const state = command === "attention_claim" ? "claimed" : command === "attention_snooze" ? "snoozed" : command === "attention_resolve" ? "resolved" : current.state;
        current = { ...current, state, version: current.version + 1 };
        return current;
      }
      return null;
    });
  });
  afterEach(cleanup);

  it("loads, filters, claims, snoozes, and opens the selected process", async () => {
    const { onOpenTask } = renderAs("operator");
    expect(await screen.findByRole("heading", { name: "Approval required" })).toBeVisible();
    expect(screen.getByText("Retry now?")).toBeVisible();
    fireEvent.change(screen.getByLabelText("Severity"), { target: { value: "intervention" } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("attention_list", expect.objectContaining({ severity: "intervention" })));
    fireEvent.click(screen.getByRole("button", { name: "Claim" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("attention_claim", expect.objectContaining({
      id: "attention-1", expected_version: 1, idempotency_key: "00000000-0000-4000-8000-000000000002",
    })));
    fireEvent.click(screen.getByRole("button", { name: "Snooze 1h" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("attention_snooze", expect.objectContaining({ id: "attention-1", expected_version: 2 })));
    fireEvent.click(screen.getByRole("button", { name: "Open process" }));
    expect(onOpenTask).toHaveBeenCalledWith("task-1");
  });

  it("confirms advertised actions and removes resolved work from the active queue", async () => {
    renderAs("operator");
    await screen.findByText("Retry now?");
    fireEvent.click(screen.getByRole("button", { name: "Escalate" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Confirm process action" })).getByRole("button", { name: "Execute reviewed action" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("attention_execute_action", expect.objectContaining({ action_id: "escalate" })));
    fireEvent.click(screen.getByRole("button", { name: "Resolve" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "Confirm resolution" })).getByRole("button", { name: "Resolve item" }));
    expect(await screen.findByText("当前没有需要处理的事项")).toBeVisible();
  });

  it("reconciles live deltas and presents stream and notification fallbacks", async () => {
    renderAs("operator", vi.fn(), false);
    await screen.findByText("Retry now?");
    expect(screen.getByRole("status")).toHaveTextContent("Desktop notifications are unavailable");
    const incoming = attention({ id: "attention-2", task_id: "task-2", title: "New blocker", severity: "attention" });
    act(() => listeners.get("attention-delta")?.({ payload: { kind: "upsert", change_id: 11, item: incoming,
      notification: { title: "New blocker", dedupe_key: "attention-2:1", attention_item_id: "attention-2", item_version: 1, severity: "attention", process_id: "task-2", deep_link: "#/attention/attention-2" } } }));
    expect(screen.getByText("New blocker")).toBeVisible();
    act(() => listeners.get("stream-error-attention")?.({ payload: "follow disconnected" }));
    expect(screen.getByRole("alert")).toHaveTextContent("follow disconnected");
    act(() => listeners.get("attention-notification-fallback")?.({ payload: "Open Attention now" }));
    expect(screen.getByRole("status")).toHaveTextContent("Open Attention now");
  });

  it("keeps read-only inspection useful without mutation actions", async () => {
    renderAs("read_only");
    await screen.findByText("Retry now?");
    expect(screen.getByText(/Read-only access/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Claim" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Escalate" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Resolve" })).toBeDisabled();
  });

  it("supports keyboard triage and opens a correlated automation route", async () => {
    current = {
      ...current,
      last_occurred_at: "2026-07-17T02:00:00Z",
    };
    const second = attention({
      id: "attention-2",
      task_id: "task-2",
      title: "Second blocker",
      source_route_id: "route-2",
      last_occurred_at: "2026-07-17T01:00:00Z",
    });
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") {
        return { items: [current, second], latest_change_id: 10 };
      }
      if (command === "attention_claim") {
        current = { ...current, state: "claimed", version: 2 };
        return current;
      }
      if (command === "attention_snooze") {
        current = { ...current, state: "snoozed", version: 3 };
        return current;
      }
      return null;
    });
    const onOpenTask = vi.fn();
    const onOpenSourceRoute = vi.fn();
    renderAs("operator", onOpenTask, true, onOpenSourceRoute);
    await screen.findByText("Second blocker");

    expect(screen.getByRole("heading", { name: "Approval required" })).toBeVisible();
    fireEvent.keyDown(document, { key: "c" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "attention_claim",
      expect.objectContaining({ id: "attention-1" }),
    ));
    fireEvent.keyDown(document, { key: "s" });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith(
      "attention_snooze",
      expect.objectContaining({ id: "attention-1" }),
    ));
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onOpenTask).toHaveBeenCalledWith("task-1");

    fireEvent.keyDown(document, { key: "j" });
    expect(screen.getByRole("heading", { name: "Second blocker" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Open automation route" }));
    expect(onOpenSourceRoute).toHaveBeenCalledWith("route-2");
  });

  it("reloads authoritative state and remains usable after a failed mutation", async () => {
    let listCalls = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") {
        listCalls += 1;
        return { items: [current], latest_change_id: 10 + listCalls };
      }
      if (command === "attention_claim") throw new Error("version conflict");
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");

    fireEvent.click(screen.getByRole("button", { name: "Claim" }));

    await waitFor(() => expect(listCalls).toBeGreaterThanOrEqual(2));
    expect(screen.getByRole("button", { name: "Claim" })).toBeEnabled();
  });
});
