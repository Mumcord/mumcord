import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useSectionOpen } from "./useSectionOpen";

// The open state is also cached in memory per id, so each test uses its own id.
const storageKey = (id: string) => `mumcord.section.open.${id}`;

describe("useSectionOpen", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses the default when nothing is stored", () => {
    const { result } = renderHook(() => useSectionOpen("default-only", false));
    expect(result.current[0]).toBe(false);
  });

  it("restores a stored open state after a restart", () => {
    localStorage.setItem(storageKey("restart"), "1");
    const { result } = renderHook(() => useSectionOpen("restart", false));
    expect(result.current[0]).toBe(true);
  });

  it("stores the new state when toggled", () => {
    const { result } = renderHook(() => useSectionOpen("toggled", true));
    act(() => {
      result.current[1](false);
    });
    expect(result.current[0]).toBe(false);
    expect(localStorage.getItem(storageKey("toggled"))).toBe("0");
  });

  it("falls back to the default when storage cannot be read", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    const { result } = renderHook(() => useSectionOpen("unreadable", true));
    expect(result.current[0]).toBe(true);
  });

  it("keeps working when storage cannot be written", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { result } = renderHook(() => useSectionOpen("unwritable", false));
    act(() => {
      result.current[1](true);
    });
    expect(result.current[0]).toBe(true);
  });
});
