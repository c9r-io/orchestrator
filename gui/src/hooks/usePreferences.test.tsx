import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useTheme } from "./useTheme";
import { useTransparency } from "./useTransparency";

describe("visual preference hooks", () => {
  const addEventListener = vi.fn();
  const removeEventListener = vi.fn();

  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    delete document.documentElement.dataset.transparency;
    addEventListener.mockReset();
    removeEventListener.mockReset();
    vi.stubGlobal("matchMedia", vi.fn(() => ({
      matches: false, media: "(prefers-color-scheme: dark)", onchange: null,
      addEventListener, removeEventListener, addListener: vi.fn(), removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })));
  });

  afterEach(() => vi.unstubAllGlobals());

  it("restores, applies, persists, and toggles an explicit theme", () => {
    localStorage.setItem("theme", "dark");
    const { result, unmount } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("dark");
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    act(() => result.current.toggleTheme());
    expect(result.current.theme).toBe("light");
    expect(document.documentElement).toHaveAttribute("data-theme", "");
    expect(localStorage.getItem("theme")).toBe("light");
    unmount();
    expect(removeEventListener).toHaveBeenCalledWith("change", expect.any(Function));
  });

  it("uses the system theme when no valid preference exists", () => {
    vi.mocked(window.matchMedia).mockReturnValue({
      matches: true, media: "(prefers-color-scheme: dark)", onchange: null,
      addEventListener, removeEventListener, addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn(),
    });
    localStorage.setItem("theme", "invalid");
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("dark");
    expect(localStorage.getItem("theme")).toBe("dark");
  });

  it("persists reduced transparency and toggles back to full", () => {
    localStorage.setItem("transparency", "reduced");
    const { result } = renderHook(() => useTransparency());
    expect(result.current.transparency).toBe("reduced");
    expect(document.documentElement.dataset.transparency).toBe("reduced");
    act(() => result.current.toggleTransparency());
    expect(result.current.transparency).toBe("full");
    expect(localStorage.getItem("transparency")).toBe("full");
  });
});
