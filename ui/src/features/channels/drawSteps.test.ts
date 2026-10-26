import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useDrawSteps } from "./drawSteps";

describe("useDrawSteps (FR-2.24)", () => {
  it("draws everything at once when not stepwise", () => {
    const { result } = renderHook(() => useDrawSteps(100, 20, false, "a"));
    expect(result.current).toBe(100);
  });

  it("adds a step per frame until everything is drawn", async () => {
    const seen: number[] = [];
    const { result } = renderHook(() => {
      const drawn = useDrawSteps(50, 20, true, "a");
      seen.push(drawn);
      return drawn;
    });
    expect(seen[0]).toBe(20);
    await waitFor(() => {
      expect(result.current).toBe(50);
    });
    expect(seen).toContain(40);
  });

  it("starts over for a new key", async () => {
    const { result, rerender } = renderHook(({ key }) => useDrawSteps(50, 20, true, key), {
      initialProps: { key: "a" },
    });
    await waitFor(() => {
      expect(result.current).toBe(50);
    });
    rerender({ key: "b" });
    expect(result.current).toBe(20);
    await waitFor(() => {
      expect(result.current).toBe(50);
    });
  });
});
