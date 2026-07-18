import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ReviewedActionDialog from "./ReviewedActionDialog";

describe("ReviewedActionDialog", () => {
  afterEach(cleanup);

  it("traps focus, requires a reason, closes on escape, and restores focus", async () => {
    const cancel = vi.fn(); const confirm = vi.fn();
    const { rerender } = render(<><button>Before</button><ReviewedActionDialog open={false} title="Replay" description="Reviewed mutation" confirmLabel="Replay route" onConfirm={confirm} onCancel={cancel} /></>);
    screen.getByRole("button", { name: "Before" }).focus();
    rerender(<><button>Before</button><ReviewedActionDialog open title="Replay" description="Reviewed mutation" confirmLabel="Replay route" onConfirm={confirm} onCancel={cancel} /></>);
    const reason = await screen.findByLabelText("Audit reason"); await waitFor(() => expect(reason).toHaveFocus());
    expect(screen.getByRole("button", { name: "Replay route" })).toBeDisabled();
    fireEvent.keyDown(document, { key: "Tab", shiftKey: true });
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" }); expect(cancel).toHaveBeenCalled();
    rerender(<><button>Before</button><ReviewedActionDialog open={false} title="Replay" description="Reviewed mutation" confirmLabel="Replay route" onConfirm={confirm} onCancel={cancel} /></>);
    expect(screen.getByRole("button", { name: "Before" })).toHaveFocus();
  });
});
