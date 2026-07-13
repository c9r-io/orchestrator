import { expect, test, type Page } from "@playwright/test";

async function installTauriMock(page: Page, role: "read_only" | "operator" = "operator") {
  await page.addInitScript(({ roleName }) => {
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number>();
    let nextId = 1;
    const items = [
      { id: "attention-1", title: "Approval required", taskId: "task-1", severity: "intervention" },
      { id: "attention-2", title: "Choose recovery", taskId: "task-2", severity: "attention" },
    ];
    const attentionRows = items.map((item) => ({
      id: item.id, project_id: "project-1", task_id: item.taskId, task_item_id: null, step_id: "test", session_id: "session-1",
      kind: "step_failed", severity: item.severity, state: "open", title: item.title, summary: "A human decision is required",
      requested_decision_json: JSON.stringify({ question: "Retry from the verified boundary?" }),
      actions: [{ id: "retry", label: "Retry safely", required_role: "operator", confirmation: "required", input_schema_json: "{}" }],
      assignee: null, occurrence_count: 1, reopen_count: 0, version: 1,
      created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z", last_occurred_at: "2026-07-14T00:00:00Z", snoozed_until: null, resolved_at: null,
    }));
    const session = { session_id: "session-1", task_id: "task-1", task_item_id: null, step_id: "test", agent_id: "coder", state: "detached", pid: 42, writer_client_id: null, writer_actor: null, writer_lease_expires_at: null, state_version: 1 };
    const invoke = async (command: string, args: Record<string, unknown> = {}) => {
      if (command === "plugin:event|listen") { listeners.set(String(args.event), Number(args.handler)); return nextId++; }
      if (command === "plugin:event|unlisten" || command.startsWith("stop_") || command.startsWith("start_")) return null;
      if (command.includes("notification")) return true;
      if (command === "connect") {
        queueMicrotask(() => { const handler = listeners.get("connection-state-changed"); if (handler) callbacks.get(handler)?.({ event: "connection-state-changed", id: 1, payload: { kind: "Connected" } }); });
        return null;
      }
      if (command === "probe_role") return roleName;
      if (command === "attention_list") return { items: attentionRows, latest_change_id: 2 };
      if (command === "attention_claim") return { ...attentionRows.find((item) => item.id === args.id), state: "claimed", version: 2 };
      if (command === "task_list") return [{ id: "task-1", name: "Fix payment failure", status: "failed", total_items: 1, finished_items: 0, failed_items: 1, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", goal: "Restore the failed payment test" }];
      if (command === "task_info") return { id: "task-1", name: "Fix payment failure", status: "failed", goal: "Restore the failed payment test", total_items: 1, finished_items: 0, failed_items: 1, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", items: [{ id: "item-1", qa_file_path: "tests/payment.rs", status: "failed", order_no: 1 }] };
      if (command === "task_timeline") return { entries: [{ id: "entry-1", task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "failure", title: "Test failed", summary: "The payment fixture assertion failed", status: "failed", actor: null, step_id: "test", task_item_id: "item-1", command_run_id: "run-1", session_id: "session-1", checkpoint_id: "checkpoint-1", source_event_id: null, evidence: [{ kind: "test", label: "cargo test payment", uri: null, content_type: "text/plain", digest: null, redacted: false }], raw_event_ids: [1], projection_version: 1 }], next_cursor: null, has_more: false, snapshot_max_event_id: 1, projection_version: 1 };
      if (command === "agent_session_list") return [session];
      if (command === "source_binding_list" || command === "source_event_list") return [];
      if (command === "resume_boundary_list") return [];
      return null;
    };
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        invoke,
        transformCallback: (callback: (payload: unknown) => void) => { const id = nextId++; callbacks.set(id, callback); return id; },
        unregisterCallback: (id: number) => callbacks.delete(id),
        convertFileSrc: (path: string) => path,
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => undefined },
    });
  }, { roleName: role });
}

test("Attention is the default and opens the semantic failed-process workspace", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Attention", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Approval required" })).toBeVisible();
  await expect(page.getByText("Autonomous background task")).toHaveCount(0);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/processes\/task-1/);
  await expect(page.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
  await expect(page.getByText("cargo test payment", { exact: true })).toBeVisible();
});

test("keyboard selection is stable and read-only mutations are disabled", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/");
  const listbox = page.getByRole("listbox");
  await expect(listbox).toHaveAttribute("aria-activedescendant", "attention-attention-1");
  await page.keyboard.press("ArrowDown");
  await expect(listbox).toHaveAttribute("aria-activedescendant", "attention-attention-2");
  await expect(page.getByRole("button", { name: "Resolve" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Retry safely" })).toBeDisabled();
});

test("narrow layout exposes the menu and reduced-transparency fallback", async ({ page }) => {
  await installTauriMock(page);
  await page.setViewportSize({ width: 640, height: 820 });
  await page.goto("/");
  await page.getByRole("button", { name: /Menu/ }).click();
  await page.getByRole("button", { name: /Reduce transparency/ }).click();
  await expect(page.locator("html")).toHaveAttribute("data-transparency", "reduced");
  await page.getByRole("link", { name: /Sessions/ }).click();
  await expect(page.getByRole("heading", { name: "Sessions" })).toBeVisible();
});
