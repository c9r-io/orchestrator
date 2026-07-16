import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import ConfirmDialog from "./ConfirmDialog";
import ConnectionBanner from "./ConnectionBanner";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("ConfirmDialog", () => {
  it("confirms, cancels from the backdrop, and keeps inner clicks inside", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { rerender } = render(<ConfirmDialog open={false} title="Delete" message="Permanent" onConfirm={onConfirm} onCancel={onCancel} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    rerender(<ConfirmDialog open title="Delete" message="Permanent" confirmLabel="Delete now" destructive onConfirm={onConfirm} onCancel={onCancel} />);
    fireEvent.click(screen.getByText("Permanent"));
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Delete now" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("presentation"));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("traps focus, closes with Escape, and restores the trigger", () => {
    const onCancel = vi.fn();
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const { rerender } = render(<ConfirmDialog open title="Resolve" message="Confirm" onConfirm={vi.fn()} onCancel={onCancel} />);
    const cancel = screen.getByRole("button", { name: "取消" });
    const confirm = screen.getByRole("button", { name: "确认" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(confirm).toHaveFocus();
    fireEvent.keyDown(document, { key: "Tab" });
    expect(cancel).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledOnce();
    rerender(<ConfirmDialog open={false} title="Resolve" message="Confirm" onConfirm={vi.fn()} onCancel={onCancel} />);
    expect(trigger).toHaveFocus();
    trigger.remove();
  });
});

describe("ConnectionBanner", () => {
  it("presents reconnect attempts and a retryable terminal failure", () => {
    const onRetry = vi.fn();
    const { rerender } = render(<ConnectionBanner state={{ kind: "Reconnecting", attempt: 2, max_attempts: 5 }} onRetry={onRetry} />);
    expect(screen.getByRole("alert")).toHaveTextContent("2/5");
    rerender(<ConnectionBanner state={{ kind: "Failed", message: "socket missing" }} onRetry={onRetry} />);
    expect(screen.getByRole("alert")).toHaveTextContent("socket missing");
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("announces restoration temporarily after a disruption", () => {
    vi.useFakeTimers();
    const { rerender } = render(<ConnectionBanner state={{ kind: "Failed", message: "offline" }} onRetry={vi.fn()} />);
    rerender(<ConnectionBanner state={{ kind: "Connected" }} onRetry={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("已恢复连接");
    act(() => vi.advanceTimersByTime(3000));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
