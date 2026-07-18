import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { RoleContext, hasAccess } from "../../hooks/useRole";
import type { Role, SourceConnection } from "../../lib/types";
import SourceConnections from "./SourceConnections";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => vi.fn()) }));

const active: SourceConnection = {
  id: "conn-installation-1", project_id: "default", provider: "slack",
  display_label: "Product Slack", provisioning_mode: "managed_shared",
  installation_id: "installation-1", installation_id_digest: "team-digest",
  app_ownership: "orchestrator", app_id_digest: null, manifest_version: null,
  provision_state: null, provision_error_code: null,
  enterprise_id_digest: null, owner_daemon_id: "daemon-1", generation: 2,
  version: 3, state: "active", capabilities: ["delivery_v1"], scopes: ["reactions:read"],
  trigger_name: "slack-installation-1", last_delivery_at: null, last_acked_cursor: 0,
  delivery_lag: 0, last_error_code: null, created_at: "2026-07-18T00:00:00Z",
  updated_at: "2026-07-18T00:00:00Z", reauthorized_at: null, disconnected_at: null,
};

function renderAs(role: Role) {
  return render(
    <RoleContext.Provider value={{ role, canAccess: (required) => hasAccess(role, required) }}>
      <SourceConnections onNavigate={vi.fn()} />
    </RoleContext.Provider>,
  );
}

describe("SourceConnections", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "source_connection_catalog_get") return {
        protocol_version: 1, gateway_configured: true, permalink_proxy: true,
        modes: [
          { mode: "managed_shared", available: true, unavailable_reason: null },
          { mode: "managed_dedicated", available: true, unavailable_reason: null },
          { mode: "manual", available: true, unavailable_reason: null },
        ],
      };
      if (command === "source_connection_list") return [active];
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command}`);
    });
  });

  afterEach(() => { cleanup(); vi.useRealTimers(); });

  it("shows all explicit provisioning modes and safe connection metadata", async () => {
    renderAs("read_only");
    expect(await screen.findByRole("heading", { name: "Instant — Official Orchestrator App" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Dedicated — Private workspace app" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Existing app — Manual credentials" })).toBeVisible();
    expect(screen.getByText("Product Slack")).toBeVisible();
    expect(screen.getByText(/generation 2/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Connect workspace" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Disconnect" })).not.toBeInTheDocument();
  });

  it("starts a resumable OAuth intent and opens only the returned authorize URL", async () => {
    const pending = {
      id: "intent-1", project_id: "default", provider: "slack", provisioning_mode: "managed_shared",
      status: "pending", connection_id: null, error_code: null,
      expires_at: "2026-07-18T01:00:00Z", authorize_url: "https://slack.com/oauth/v2/authorize?state=opaque",
      connection: null,
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "source_connection_catalog_get") return { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_shared", available: true, unavailable_reason: null }] };
      if (command === "source_connection_list") return [];
      if (command === "source_connection_connect") return pending;
      if (command === "open_source_connection_oauth") return true;
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command}`);
    });
    renderAs("admin");
    await screen.findByRole("button", { name: "Connect workspace" });
    fireEvent.change(screen.getByLabelText("Connection label"), { target: { value: "Engineering" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect workspace" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_connection_connect", expect.objectContaining({ project_id: "default", display_label: "Engineering" })));
    expect(invoke).toHaveBeenCalledWith("open_source_connection_oauth", { authorize_url: pending.authorize_url });
    expect(JSON.parse(localStorage.getItem("orchestrator.sourceConnectionIntent.v1") ?? "null")).toEqual({ id: "intent-1", project: "default" });
    expect(await screen.findByText("Waiting for Slack consent")).toBeVisible();
  });

  it("clears the Configuration Token before review and persists only a safe checkpoint", async () => {
    const preview = {
      id: "dedicated-1", project_id: "default", status: "awaiting_approval",
      manifest_version: "v1", manifest_digest: "a".repeat(64), app_id_digest: null,
      oauth_intent_id: null, authorize_url: null, error_code: null,
      expires_at: "2026-07-18T01:00:00Z",
      diff: [{ field: "bot_scopes", change: "changed", before: [], after: ["reactions:read"], permission_expansion: true }],
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "source_connection_catalog_get") return { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_dedicated", available: true, unavailable_reason: null }] };
      if (command === "source_connection_list") return [];
      if (command === "source_connection_dedicated_preview") {
        expect(args).toEqual(expect.objectContaining({ config_token: "xoxe.xoxp-sensitive-token", display_label: "Private Engineering" }));
        return preview;
      }
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command}`);
    });
    renderAs("admin");
    fireEvent.change(await screen.findByLabelText("Dedicated connection label"), { target: { value: "Private Engineering" } });
    const token = screen.getByLabelText("One-time Configuration Token") as HTMLInputElement;
    expect(token.type).toBe("password");
    expect(token.autocomplete).toBe("off");
    fireEvent.change(token, { target: { value: "xoxe.xoxp-sensitive-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Validate manifest" }));
    await screen.findByRole("heading", { name: "Dedicated app provisioning review" });
    expect(token.value).toBe("");
    expect(screen.getByText(/permission expansion/)).toBeVisible();
    const retained = localStorage.getItem("orchestrator.dedicatedSlackProvisioning.v1") ?? "";
    expect(JSON.parse(retained)).toEqual({ id: "dedicated-1", project: "default" });
    expect(retained).not.toContain("sensitive-token");
    expect(document.body.textContent).not.toContain("xoxe.xoxp-sensitive-token");
  });

  it("requires a second reviewed approval before creating a dedicated App", async () => {
    const preview = {
      id: "dedicated-2", project_id: "default", status: "awaiting_approval",
      manifest_version: "v1", manifest_digest: "b".repeat(64), app_id_digest: null,
      oauth_intent_id: null, authorize_url: null, error_code: null,
      expires_at: "2026-07-18T01:00:00Z",
      diff: [{ field: "events", change: "changed", before: [], after: ["reaction_added"], permission_expansion: true }],
    };
    const approved = {
      ...preview, status: "oauth_pending", diff: [], oauth_intent_id: "intent-dedicated-2",
      authorize_url: "https://slack.com/oauth/v2/authorize?state=dedicated",
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "source_connection_catalog_get") return { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_dedicated", available: true, unavailable_reason: null }] };
      if (command === "source_connection_list") return [];
      if (command === "source_connection_dedicated_preview") return preview;
      if (command === "source_connection_dedicated_approve") return approved;
      if (command === "source_connection_intent_get") return { id: "intent-dedicated-2", project_id: "default", provider: "slack", provisioning_mode: "managed_dedicated", status: "pending", connection_id: null, error_code: null, expires_at: approved.expires_at, authorize_url: approved.authorize_url, connection: null };
      if (command === "open_source_connection_oauth") return true;
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command}`);
    });
    renderAs("admin");
    fireEvent.change(await screen.findByLabelText("One-time Configuration Token"), { target: { value: "one-time" } });
    fireEvent.click(screen.getByRole("button", { name: "Validate manifest" }));
    fireEvent.click(await screen.findByRole("button", { name: "Approve and create app" }));
    const dialog = await screen.findByRole("dialog", { name: "Create dedicated Slack App" });
    expect(within(dialog).getByRole("button", { name: "Create app" })).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("Audit reason"), { target: { value: "isolate regulated workspace" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create app" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_connection_dedicated_approve", expect.objectContaining({ provisioning_id: "dedicated-2", reason: "isolate regulated workspace" })));
    expect(invoke).toHaveBeenCalledWith("open_source_connection_oauth", { authorize_url: approved.authorize_url });
  });

  it("requires a reviewed reason and sends the displayed version fence when disconnecting", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "source_connection_catalog_get") return { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_shared", available: true, unavailable_reason: null }] };
      if (command === "source_connection_list") return [active];
      if (command === "source_connection_disconnect") return { ...active, state: "disconnected", version: 4 };
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
    });
    renderAs("admin");
    fireEvent.click(await screen.findByRole("button", { name: "Disconnect" }));
    const dialog = await screen.findByRole("dialog", { name: "Disconnect Product Slack" });
    fireEvent.change(screen.getByLabelText("Audit reason"), { target: { value: "retire obsolete connection" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_connection_disconnect", expect.objectContaining({ id: active.id, expected_version: 3, reason: "retire obsolete connection" })));
  });

  it("transfers to a different daemon with the displayed version and reviewed reason", async () => {
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "source_connection_catalog_get") return { protocol_version: 1, gateway_configured: true, permalink_proxy: true, modes: [{ mode: "managed_shared", available: true, unavailable_reason: null }] };
      if (command === "source_connection_list") return [active];
      if (command === "source_connection_transfer") return { ...active, state: "suspended", version: 4, owner_daemon_id: "daemon-2" };
      if (["start_source_connection_watch", "stop_source_connection_watch"].includes(command)) return null;
      throw new Error(`unexpected ${command} ${JSON.stringify(args)}`);
    });
    renderAs("admin");
    fireEvent.click(await screen.findByRole("button", { name: "Transfer" }));
    const dialog = await screen.findByRole("dialog", { name: "Transfer Product Slack" });
    fireEvent.change(within(dialog).getByLabelText("Target daemon ID"), { target: { value: "daemon-2" } });
    fireEvent.change(within(dialog).getByLabelText("Audit reason"), { target: { value: "move to replacement daemon" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Transfer ownership" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("source_connection_transfer", expect.objectContaining({ id: active.id, expected_version: 3, target_daemon_id: "daemon-2", reason: "move to replacement daemon" })));
  });
});
