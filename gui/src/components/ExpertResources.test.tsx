import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ExpertResources from "./ExpertResources";
import { RoleContext, hasAccess } from "../hooks/useRole";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("ExpertResources", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());
  afterEach(cleanup);

  it("switches resource kinds and clears stale content when loading fails", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ content: "kind: workflows", format: "yaml" })
      .mockRejectedValueOnce(new Error("resource backend unavailable"));
    render(
      <RoleContext.Provider
        value={{
          role: "read_only",
          canAccess: (required) => hasAccess("read_only", required),
        }}
      >
        <ExpertResources />
      </RoleContext.Provider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "workflows" }));
    expect(await screen.findByText("kind: workflows")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("resource_get", {
      resource: "workflows",
      outputFormat: "yaml",
    });

    fireEvent.click(screen.getByRole("button", { name: "agents" }));

    expect(await screen.findByText("Error: resource backend unavailable")).toBeVisible();
    await waitFor(() =>
      expect(screen.queryByText("kind: workflows")).not.toBeInTheDocument(),
    );
  });
});
