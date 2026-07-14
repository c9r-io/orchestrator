import { expect, test, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

async function installTauriMock(page: Page, role: "read_only" | "operator" = "operator") {
  await page.addInitScript(({ roleName }) => {
    const callbacks = new Map<number, (payload: unknown) => void>();
    const listeners = new Map<string, number>();
    const sessionCalls: Array<{ command: string; args: Record<string, unknown> }> = [];
    const processCalls: Array<{ command: string; args: Record<string, unknown> }> = [];
    let nextId = 1;
    const items = [
      { id: "attention-1", title: "Approval required", taskId: "task-1", severity: "intervention" },
      { id: "attention-2", title: "Choose recovery", taskId: "task-2", severity: "attention" },
    ];
    const attentionRows = items.map((item) => ({
      id: item.id, project_id: "project-1", task_id: item.taskId, task_item_id: null, step_id: "test", session_id: "session-1",
      kind: "step_failed", severity: item.severity, state: "open", title: item.title, summary: "A human decision is required",
      requested_decision_json: JSON.stringify({ question: "Retry from the verified boundary?" }),
      actions: [{ id: "retry_failed_item", label: "Retry safely", required_role: "operator", confirmation: "required", input_schema_json: "{}" }],
      assignee: null, occurrence_count: 1, reopen_count: 0, version: 1,
      created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z", last_occurred_at: "2026-07-14T00:00:00Z", snoozed_until: null, resolved_at: null,
    }));
    let session = { session_id: "session-1", task_id: "task-1", task_item_id: null, step_id: "test", agent_id: "coder", state: "detached", pid: 42, writer_client_id: null as string | null, writer_actor: null as string | null, writer_lease_expires_at: null as string | null, state_version: 1 };
    const invoke = async (command: string, args: Record<string, unknown> = {}) => {
      processCalls.push({ command, args });
      if (command === "plugin:event|listen") { listeners.set(String(args.event), Number(args.handler)); return nextId++; }
      if (command === "plugin:event|unlisten") return null;
      if (command.startsWith("start_agent_session") || command.startsWith("stop_agent_session")) { sessionCalls.push({ command, args }); return null; }
      if (command.includes("notification")) return true;
      if (command === "connect") {
        queueMicrotask(() => { const handler = listeners.get("connection-state-changed"); if (handler) callbacks.get(handler)?.({ event: "connection-state-changed", id: 1, payload: { kind: "Connected" } }); });
        return null;
      }
      if (command === "probe_role") return roleName;
      if (command === "process_metric_record") return true;
      if (command === "process_metrics_get") {
        const metric = (name: string, value: number, sampleCount = 1, extra: Record<string, unknown> = {}) => ({
          name, labels: {}, sample_count: sampleCount, sum: value, min: value, max: value,
          value, numerator: null, denominator: null, histogram: {}, buckets: [], ...extra,
        });
        return {
          schema_version: 1, project_id: String(args.project_id),
          window_start: "2026-07-13T00:00:00Z", window_end: "2026-07-14T00:00:00Z",
          generated_at: new Date().toISOString(), coverage_start: "2026-07-01T00:00:00Z",
          partial: false, collection_enabled: true,
          metrics: [
            metric("attention_open_total", 4, 4), metric("attention_active", 2, 2),
            metric("attention_time_to_claim_seconds", 12, 2), metric("process_human_attention_seconds", 60, 2),
            metric("process_autonomous_completion_ratio", 0.75, 0, { numerator: 3, denominator: 4 }),
            metric("handoff_to_productive_action_seconds", 20), metric("resume_attempt_total", 2, 2, { labels: { mode: "restart_step", result: "succeeded" } }),
            metric("session_attachment_total", 3, 3, { labels: { mode: "writer", result: "succeeded" } }),
            metric("source_event_deduplicated_total", 2, 2, { labels: { provider: "slack" } }),
            metric("process_repeated_failure_rate", 0.25, 0, { numerator: 1, denominator: 4 }),
            metric("process_degenerate_loop_rate", 0.1, 0, { numerator: 1, denominator: 10 }),
            metric("timeline_projection_seconds", 0.08, 2), metric("timeline_response_bytes", 4096, 2),
            metric("stream_reconnect_total", 1),
          ],
          projector_health: [{ projector: "attention", project_id: "", cursor: "42", lag_count: 0, failure_count: 0, last_error_code: null, last_success_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:00:00Z" }],
        };
      }
      if (command === "attention_list") return { items: attentionRows, latest_change_id: 2 };
      if (command === "attention_claim") return { ...attentionRows.find((item) => item.id === args.id), state: "claimed", version: 2 };
      if (command === "task_list") return [{ id: "task-1", name: "Fix payment failure", status: "failed", total_items: 1, finished_items: 0, failed_items: 1, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", goal: "Restore the failed payment test" }];
      if (command === "task_info") return { id: "task-1", name: "Fix payment failure", status: "failed", goal: "Restore the failed payment test", total_items: 1, finished_items: 0, failed_items: 1, created_at: "2026-07-14T00:00:00Z", updated_at: "2026-07-14T00:01:00Z", project_id: "project-1", workflow_id: "qa-loop", items: [{ id: "item-1", qa_file_path: "tests/payment.rs", status: "failed", order_no: 1 }] };
      if (command === "task_timeline") return { entries: [{ id: "entry-1", task_id: "task-1", occurred_at: "2026-07-14T00:00:00Z", category: "failure", title: "Test failed", summary: "The payment fixture assertion failed", status: "failed", actor: null, step_id: "test", task_item_id: "item-1", command_run_id: "run-1", session_id: "session-1", checkpoint_id: "checkpoint-1", source_event_id: null, evidence: [{ kind: "test", label: "cargo test payment", uri: null, content_type: "text/plain", digest: null, redacted: false }], raw_event_ids: [1], projection_version: 1 }], next_cursor: null, has_more: false, snapshot_max_event_id: 1, projection_version: 1 };
      if (command === "agent_session_list") return [session];
      if (command === "agent_session_attach") {
        sessionCalls.push({ command, args });
        session = { ...session, state: "active", writer_client_id: String(args.client_id), writer_actor: "operator", writer_lease_expires_at: "2026-07-14T00:01:00Z", state_version: session.state_version + 1 };
        return { fencing_token: 7, lease_expires_at: session.writer_lease_expires_at };
      }
      if (command === "agent_session_heartbeat") { sessionCalls.push({ command, args }); return "2026-07-14T00:02:00Z"; }
      if (command === "agent_session_send_input") { sessionCalls.push({ command, args }); return String(args.text).length; }
      if (command === "agent_session_detach") {
        sessionCalls.push({ command, args });
        session = { ...session, state: "detached", writer_client_id: null, writer_actor: null, writer_lease_expires_at: null, state_version: session.state_version + 1 };
        return true;
      }
      if (command === "agent_session_close") { sessionCalls.push({ command, args }); session = { ...session, state: "draining", state_version: session.state_version + 1 }; return session; }
      if (command === "source_binding_list" || command === "source_event_list") return [];
      if (command === "resume_boundary_list") return [{ id: "boundary-1", task_id: "task-1", cycle: 1, step_id: "test", task_item_id: "item-1", provider_session_available: false, side_effect_class: "workspace_only", replay_safe: true, reason: "Failed step can be replayed", state_version: "state-1" }];
      if (command === "resume_plan") return { id: "plan-1", task_id: "task-1", boundary: null, mode: String(args.mode), expected_state_version: "state-1", consequence: { repeated_steps: ["test"], workspace_rollback: false }, elevated_confirmation_required: false, expires_at: "2026-07-14T01:00:00Z", status: "review_required" };
      if (command === "resume_execute") return { execution_id: "execution-1", plan_id: "plan-1", accepted: true, status: "succeeded", child_task_id: "task-child" };
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
      __SESSION_TEST__: {
        calls: sessionCalls,
        emit: (event: string, payload: unknown) => {
          const handler = listeners.get(event);
          if (handler) callbacks.get(handler)?.({ event, id: 1, payload });
        },
      },
      __PROCESS_TEST__: { calls: processCalls },
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
  await expect(page.getByRole("button", { name: "Retry safely" })).toHaveCount(0);
});

test("failed process uses reviewed resume and never routes the primary action to orphan repair", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/processes/task-1");
  const review = page.getByRole("button", { name: "Review safe resume" });
  await review.click();
  const dialog = page.getByRole("dialog", { name: "Resume consequence preview" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Create preview" }).click();
  await dialog.getByLabel("Operator reason").fill("Reviewed the failed test evidence");
  await dialog.getByRole("button", { name: "Execute reviewed plan" }).click();
  await expect(dialog.getByRole("status")).toContainText("succeeded");
  const commands = await page.evaluate(() => (window as any).__PROCESS_TEST__.calls.map((call: any) => call.command));
  expect(commands).toEqual(expect.arrayContaining(["resume_boundary_list", "resume_plan", "resume_execute"]));
  expect(commands).not.toContain("task_recover");
  await dialog.getByRole("button", { name: "Close resume dialog" }).click();
  await expect(review).toBeFocused();
});

test("confirmation dialogs trap focus, close with Escape, and restore focus", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  const resolve = page.getByRole("button", { name: "Resolve" });
  await resolve.click();
  const dialog = page.getByRole("dialog", { name: "Confirm resolution" });
  const cancel = dialog.getByRole("button", { name: "取消" });
  const confirm = dialog.getByRole("button", { name: "Resolve item" });
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(confirm).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(resolve).toBeFocused();
});

test("Attention and failed-process workspace have no serious axe violations", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
  await page.goto("/#/processes/task-1");
  await expect(page.getByRole("heading", { name: "Fix payment failure" })).toBeVisible();
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("reduced-motion preference suppresses UI transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await installTauriMock(page);
  await page.goto("/");
  const duration = await page.getByRole("button", { name: "Refresh snapshot" }).evaluate((element) => getComputedStyle(element).transitionDuration);
  expect(Number.parseFloat(duration)).toBeLessThanOrEqual(0.00001);
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

test("read-only Operations renders process health, switches windows, and remains accessible", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/system/operations");
  await expect(page.getByRole("heading", { name: "Operations" })).toBeVisible();
  await expect(page.getByText("75%")).toBeVisible();
  await expect(page.getByText("Fresh snapshot")).toBeVisible();
  await expect(page.getByRole("cell", { name: "attention" })).toBeVisible();
  await page.getByRole("button", { name: "7 days" }).click();
  await expect.poll(() => page.evaluate(() => {
    const calls = (window as any).__PROCESS_TEST__.calls.filter((call: any) => call.command === "process_metrics_get");
    return calls.at(-1)?.args.window;
  })).toBe("7d");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});

test("session inspector commits offsets, controls one writer, and links to its process", async ({ page }) => {
  await installTauriMock(page);
  await page.goto("/#/sessions");
  await page.getByRole("listitem").click();
  await expect(page.getByRole("heading", { name: "Session inspector" })).toBeVisible();

  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("agent-session-output-session-1", {
    offset: 0, next_offset: 5, text: "hello", eof: false, redacted: false,
  }));
  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("agent-session-output-session-1", {
    offset: 0, next_offset: 5, text: "duplicate", eof: false, redacted: false,
  }));
  await expect(page.getByRole("log")).toHaveText("hello");

  await page.evaluate(() => (window as any).__SESSION_TEST__.emit("stream-error-agent-session-session-1", "disconnected"));
  await expect.poll(() => page.evaluate(() => {
    const starts = (window as any).__SESSION_TEST__.calls.filter((call: any) => call.command === "start_agent_session_read");
    return starts.at(-1)?.args.offset;
  })).toBe(5);

  await page.getByRole("button", { name: "Request control" }).click();
  await page.getByLabel("Session input").fill("hello-agent");
  await page.getByRole("button", { name: "Send" }).click();
  await page.getByRole("button", { name: "Release control" }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__SESSION_TEST__.calls.map((call: any) => call.command)))
    .toEqual(expect.arrayContaining(["agent_session_attach", "agent_session_send_input", "agent_session_detach"]));

  await page.getByRole("button", { name: "Open linked process" }).click();
  await expect(page).toHaveURL(/#\/processes\/task-1/);
});

test("read-only session inspector has no focusable mutation controls", async ({ page }) => {
  await installTauriMock(page, "read_only");
  await page.goto("/#/sessions");
  await page.getByRole("listitem").click();
  await expect(page.getByText(/Read-only access/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Request control" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Close session" })).toHaveCount(0);
  await expect(page.getByLabel("Session input")).toHaveCount(0);
});
