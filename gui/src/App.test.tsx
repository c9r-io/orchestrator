import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { isPermissionGranted, onAction, requestPermission } from "@tauri-apps/plugin-notification";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { useConnectionState } from "./hooks/useConnectionState";
import { useTheme } from "./hooks/useTheme";
import { useTransparency } from "./hooks/useTransparency";
import { featureEnabled } from "./lib/features";
import { useConsoleRoute } from "./lib/routes";
import { recordUiMetric } from "./lib/telemetry";
import type { ConsoleRoute } from "./lib/routes";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(), onAction: vi.fn(), requestPermission: vi.fn() }));
vi.mock("./hooks/useConnectionState", () => ({ useConnectionState: vi.fn() }));
vi.mock("./hooks/useTheme", () => ({ useTheme: vi.fn() }));
vi.mock("./hooks/useTransparency", () => ({ useTransparency: vi.fn() }));
vi.mock("./lib/features", () => ({ featureEnabled: vi.fn() }));
vi.mock("./lib/routes", () => ({ useConsoleRoute: vi.fn() }));
vi.mock("./lib/telemetry", () => ({ recordUiMetric: vi.fn() }));
vi.mock("./components/ConnectionBanner", () => ({ default: ({ state }: { state: { kind: string } }) => <div>banner {state.kind}</div> }));
vi.mock("./pages/ConnectionStatus", () => ({ default: ({ state }: { state: { kind: string } }) => <div>connection {state.kind}</div> }));
vi.mock("./pages/AttentionInbox", () => ({ default: ({ nativeNotificationsEnabled, onOpenTask }: { nativeNotificationsEnabled: boolean; onOpenTask: (id: string) => void }) => <button onClick={() => onOpenTask("task-attention")}>attention page {String(nativeNotificationsEnabled)}</button> }));
vi.mock("./pages/ProcessList", () => ({ default: ({ onSelect }: { onSelect: (id: string) => void }) => <button onClick={() => onSelect("task-list")}>process list</button> }));
vi.mock("./pages/ProcessWorkspace", () => ({ default: ({ taskId, onBack }: { taskId: string; onBack: () => void }) => <button onClick={onBack}>process workspace {taskId}</button> }));
vi.mock("./pages/SessionList", () => ({ default: ({ onSelect }: { onSelect: (id: string) => void }) => <button onClick={() => onSelect("session-list")}>session list</button> }));
vi.mock("./pages/SessionInspector", () => ({ default: ({ sessionId, onOpenProcess }: { sessionId: string; onOpenProcess: (id: string) => void }) => <button onClick={() => onOpenProcess("task-session")}>session inspector {sessionId}</button> }));
vi.mock("./pages/Sources", () => ({ default: ({ onOpenTask }: { onOpenTask: (id: string) => void }) => <button onClick={() => onOpenTask("task-source")}>sources page</button> }));
vi.mock("./pages/System", () => ({ default: ({ initialSection }: { initialSection?: string }) => <div>system {initialSection}</div> }));
vi.mock("./pages/WishPool", () => ({ default: ({ onSelectWish }: { onSelectWish: (id: string) => void }) => <button onClick={() => onSelectWish("draft-1")}>wish pool</button> }));
vi.mock("./pages/WishDetail", () => ({ default: ({ taskId, onConfirmed }: { taskId: string; onConfirmed: (id: string) => void }) => <button onClick={() => onConfirmed("task-confirmed")}>wish detail {taskId}</button> }));

const navigate = vi.fn();
const reconnect = vi.fn();
const toggleTheme = vi.fn();
const toggleTransparency = vi.fn();
let route: ConsoleRoute = { page: "attention" };
let notificationAction: ((notification: { extra?: Record<string, unknown> }) => void) | undefined;

describe("App shell", () => {
  beforeEach(() => {
    route = { page: "attention" };
    navigate.mockReset();
    reconnect.mockReset();
    toggleTheme.mockReset();
    toggleTransparency.mockReset();
    notificationAction = undefined;
    vi.mocked(invoke).mockImplementation(async (command) => command === "probe_role" ? "operator" : null);
    vi.mocked(isPermissionGranted).mockResolvedValue(false);
    vi.mocked(requestPermission).mockResolvedValue("granted");
    vi.mocked(onAction).mockImplementation(async (handler) => {
      notificationAction = handler as (notification: { extra?: Record<string, unknown> }) => void;
      return { unregister: vi.fn() } as never;
    });
    vi.mocked(useConnectionState).mockReturnValue({ connectionState: { kind: "Connected" }, reconnect });
    vi.mocked(useTheme).mockReturnValue({ theme: "light", toggleTheme });
    vi.mocked(useTransparency).mockReturnValue({ transparency: "full", toggleTransparency });
    vi.mocked(useConsoleRoute).mockImplementation(() => ({ route, navigate }));
    vi.mocked(featureEnabled).mockReturnValue(true);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { callback(0); return 1; });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("shows connection recovery instead of the shell while disconnected", () => {
    vi.mocked(useConnectionState).mockReturnValue({ connectionState: { kind: "Failed", message: "offline" }, reconnect });
    render(<App />);
    expect(screen.getByText("banner Failed")).toBeVisible();
    expect(screen.getByText("connection Failed")).toBeVisible();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("renders visible navigation, role, preferences, shortcuts, and Attention handoff", async () => {
    render(<App />);
    expect(screen.getByRole("navigation", { name: "主导航" }).getElementsByTagName("a")).toHaveLength(5);
    expect(await screen.findByText("operator")).toBeVisible();
    expect(screen.getByRole("button", { name: /attention page true/ })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "切换到深色模式" }));
    fireEvent.click(screen.getByRole("button", { name: /Reduce transparency/ }));
    expect(toggleTheme).toHaveBeenCalledOnce();
    expect(toggleTransparency).toHaveBeenCalledOnce();
    fireEvent.keyDown(document, { key: "3", ctrlKey: true });
    fireEvent.keyDown(document, { key: "n", metaKey: true });
    fireEvent.click(screen.getByRole("button", { name: /attention page true/ }));
    expect(navigate.mock.calls).toEqual(expect.arrayContaining([
      [{ page: "sessions" }], [{ page: "new-process" }], [{ page: "processes", taskId: "task-attention" }],
    ]));
    expect(recordUiMetric).toHaveBeenCalledWith("page_load", expect.objectContaining({ page: "attention" }));
  });

  it("maps stable resource routes to their integrated destinations", () => {
    route = { page: "sessions", sessionId: "session-1" };
    const view = render(<App />);
    fireEvent.click(screen.getByRole("button", { name: /session inspector session-1/ }));
    expect(navigate).toHaveBeenCalledWith({ page: "processes", taskId: "task-session" });
    route = { page: "sources", taskId: "task-source" };
    view.rerender(<App />);
    expect(screen.getByRole("button", { name: /process workspace task-source/ })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /process workspace task-source/ }));
    expect(navigate).toHaveBeenCalledWith({ page: "sources" });
    route = { page: "new-process", draftId: "draft-1" };
    view.rerender(<App />);
    fireEvent.click(screen.getByRole("button", { name: /wish detail draft-1/ }));
    expect(navigate).toHaveBeenCalledWith({ page: "processes", taskId: "task-confirmed" });
  });

  it("fails closed for disabled pages and accepts only safe notification deep links", async () => {
    vi.mocked(featureEnabled).mockImplementation((feature) => feature !== "system");
    route = { page: "system", section: "operations" };
    render(<App />);
    expect(screen.getByRole("heading", { name: "Feature unavailable" })).toBeVisible();
    await waitFor(() => expect(notificationAction).toBeTypeOf("function"));
    notificationAction?.({ extra: { deep_link: "#/processes/task-1" } });
    expect(window.location.hash).toBe("#/processes/task-1");
    notificationAction?.({ extra: { deep_link: "https://malicious.example" } });
    expect(window.location.hash).toBe("#/processes/task-1");
  });
});
