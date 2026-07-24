import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AttentionInbox from "./AttentionInbox";
import { RoleContext, hasAccess } from "../hooks/useRole";
import { recordUiMetric } from "../lib/telemetry";
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

const safeError = (category: string, request_id: string | null = null) => ({
  category,
  message: "provider token=must-not-render",
  request_id,
});

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

  it("routes retry and resume actions into reviewed safe resume without executing them", async () => {
    current = attention({
      actions: [{
        id: "retry_failed_item",
        label: "Retry safely",
        required_role: "operator",
        confirmation: "required",
        input_schema_json: "{}",
      }],
    });
    const { onOpenTask } = renderAs("operator");
    await screen.findByText("Retry now?");
    vi.mocked(invoke).mockClear();

    expect(screen.queryByRole("button", { name: "Retry safely" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review safe resume" }));

    expect(onOpenTask).toHaveBeenCalledWith("task-1", true);
    expect(invoke).not.toHaveBeenCalledWith("attention_execute_action", expect.anything());
  });

  it("reconciles live deltas and presents stream and notification fallbacks", async () => {
    renderAs("operator", vi.fn(), false);
    await screen.findByText("Retry now?");
    expect(screen.getByRole("status")).toHaveTextContent("Desktop notifications are unavailable");
    const incoming = attention({ id: "attention-2", task_id: "task-2", title: "New blocker", severity: "attention" });
    act(() => listeners.get("attention-delta")?.({ payload: { kind: "upsert", change_id: 11, item: incoming,
      notification: { title: "New blocker", dedupe_key: "attention-2:1", attention_item_id: "attention-2", item_version: 1, severity: "attention", process_id: "task-2", deep_link: "#/attention/attention-2" } } }));
    expect(screen.getByText("New blocker")).toBeVisible();
    act(() => listeners.get("stream-error-attention")?.({ payload: safeError("unavailable") }));
    expect(screen.getAllByRole("status").some((node) => node.textContent?.includes("Live updates are disconnected"))).toBe(true);
    act(() => listeners.get("attention-notification-fallback")?.({ payload: "Open Attention now" }));
    expect(screen.getAllByRole("status").some((node) => node.textContent?.includes("Open Attention now"))).toBe(true);
  });

  it("keeps read-only inspection useful without mutation actions", async () => {
    renderAs("read_only");
    await screen.findByText("Retry now?");
    expect(screen.getByText(/Read-only access/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Claim" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Escalate" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Resolve" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Review safe resume" })).not.toBeInTheDocument();
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

  it("keeps a conflict visible while restoring the authoritative item and focus", async () => {
    let listCalls = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") {
        listCalls += 1;
        return { items: [current], latest_change_id: 10 + listCalls };
      }
      if (command === "attention_claim") {
        current = { ...current, state: "claimed", assignee: "operator-b", version: 2 };
        throw safeError("conflict", "req-conflict-121");
      }
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");

    const claim = screen.getByRole("button", { name: "Claim" });
    claim.focus();
    fireEvent.click(claim);

    await waitFor(() => expect(listCalls).toBeGreaterThanOrEqual(2));
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Claim failed for Approval required");
    expect(alert).toHaveTextContent("latest daemon state has been restored");
    expect(alert).toHaveTextContent("req-conflict-121");
    expect(alert).not.toHaveTextContent("must-not-render");
    expect(screen.getByText("operator-b")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Claim" })).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("listbox")).toHaveFocus());
    expect(recordUiMetric).toHaveBeenCalledWith("attention_mutation", expect.objectContaining({
      action: "claim", result: "failure", error_category: "conflict",
    }));
    expect(recordUiMetric).toHaveBeenCalledWith("attention_reconciliation", expect.objectContaining({
      action: "claim", result: "confirmed",
    }));
  });

  it("preserves both mutation and unconfirmed-state errors when reconciliation also fails", async () => {
    let listCalls = 0;
    let allowReconciliation = false;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") {
        listCalls += 1;
        if (listCalls > 1 && !allowReconciliation) throw safeError("unavailable");
        return { items: [current], latest_change_id: 10 + listCalls };
      }
      if (command === "attention_claim") throw safeError("conflict");
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");
    fireEvent.click(screen.getByRole("button", { name: "Claim" }));

    await screen.findByText(/Latest state is not confirmed/);
    expect(screen.getAllByRole("alert").some((node) => node.textContent?.includes("Claim failed"))).toBe(true);
    expect(screen.getByText("Latest Attention state could not be loaded.")).toBeVisible();
    expect(screen.queryByText(/succeeded for Approval/)).not.toBeInTheDocument();

    allowReconciliation = true;
    fireEvent.click(screen.getByRole("button", { name: "Retry latest state check" }));
    await waitFor(() => expect(screen.queryByText(/Claim failed/)).not.toBeInTheDocument());
    expect(screen.queryByText("Latest Attention state could not be loaded.")).not.toBeInTheDocument();
    expect(screen.getByText(/Latest state confirmed/)).toBeInTheDocument();
  });

  it("clears a prior error on same-operation success and never reuses its idempotency key", async () => {
    const keys: string[] = [];
    let attempts = 0;
    vi.spyOn(crypto, "randomUUID")
      .mockReturnValueOnce("00000000-0000-4000-8000-000000000021")
      .mockReturnValueOnce("00000000-0000-4000-8000-000000000022");
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "attention_list") return { items: [current], latest_change_id: 10 };
      if (command === "attention_claim") {
        attempts += 1;
        keys.push(String((args as Record<string, unknown>).idempotency_key));
        if (attempts === 1) throw safeError("conflict");
        current = { ...current, state: "claimed", assignee: "operator-a", version: 2 };
        return current;
      }
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");

    fireEvent.click(screen.getByRole("button", { name: "Claim" }));
    await screen.findByText(/Claim failed/);
    fireEvent.click(screen.getByRole("button", { name: "Claim" }));

    await waitFor(() => expect(screen.queryByText(/Claim failed/)).not.toBeInTheDocument());
    expect(keys).toEqual([
      "00000000-0000-4000-8000-000000000021",
      "00000000-0000-4000-8000-000000000022",
    ]);
    expect(screen.getByText(/claim succeeded for Approval required/)).toBeInTheDocument();
  });

  it("dismisses a persistent mutation error without changing the restored item", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") return { items: [current], latest_change_id: 10 };
      if (command === "attention_claim") throw safeError("conflict");
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");
    fireEvent.click(screen.getByRole("button", { name: "Claim" }));
    await screen.findByText(/Claim failed/);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss claim error" }));
    expect(screen.queryByText(/Claim failed/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Claim" })).toBeEnabled();
  });

  it.each([
    ["claim", "attention_claim"],
    ["snooze", "attention_snooze"],
    ["resolve", "attention_resolve"],
    ["execute", "attention_execute_action"],
  ] as const)("uses the shared failure/reconciliation contract for %s", async (operation, failedCommand) => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "attention_list") return { items: [current], latest_change_id: 10 };
      if (command === failedCommand) throw safeError("conflict");
      return null;
    });
    renderAs("operator");
    await screen.findByText("Retry now?");

    if (operation === "claim") fireEvent.click(screen.getByRole("button", { name: "Claim" }));
    if (operation === "snooze") fireEvent.click(screen.getByRole("button", { name: "Snooze 1h" }));
    if (operation === "resolve") {
      fireEvent.click(screen.getByRole("button", { name: "Resolve" }));
      fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Resolve item" }));
    }
    if (operation === "execute") {
      fireEvent.click(screen.getByRole("button", { name: "Escalate" }));
      fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Execute reviewed action" }));
    }

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("latest daemon state has been restored");
    expect(recordUiMetric).toHaveBeenCalledWith("attention_mutation", expect.objectContaining({
      action: operation, result: "failure",
    }));
  });
});
