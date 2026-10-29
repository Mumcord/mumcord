import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { originalSrc, useDisplaySrc } from "./displaySrc";

const GIF = "data:image/gif;base64,R0lGODlhAQABAAAAACw=";
const PNG = "data:image/png;base64,iVBORw0KGgo=";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useDisplaySrc", () => {
  it("shows a data: URL through a blob: URL of the same bytes", async () => {
    const { result } = renderHook(() => useDisplaySrc(GIF));
    const shown = result.current ?? "";
    expect(shown).toMatch(/^blob:/);
    expect(originalSrc(shown)).toBe(GIF);
    const blob = await (await fetch(shown)).blob();
    expect(blob.type).toBe("image/gif");
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(
      Uint8Array.from(atob("R0lGODlhAQABAAAAACw="), (c) => c.charCodeAt(0)),
    );
  });

  it("revokes the blob: URL when the image changes and when it unmounts", () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const { result, rerender, unmount } = renderHook(({ src }) => useDisplaySrc(src), {
      initialProps: { src: GIF },
    });
    const first = result.current ?? "";
    rerender({ src: PNG });
    const second = result.current ?? "";
    expect(second).not.toBe(first);
    expect(revoke).toHaveBeenCalledWith(first);
    expect(originalSrc(first)).toBe(first);
    expect(originalSrc(second)).toBe(PNG);
    unmount();
    expect(revoke).toHaveBeenCalledWith(second);
    expect(originalSrc(second)).toBe(second);
  });

  it("keeps a shared blob: URL until the last image showing it unmounts", () => {
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    const one = renderHook(() => useDisplaySrc(GIF));
    const two = renderHook(() => useDisplaySrc(GIF));
    const shown = one.result.current;
    expect(two.result.current).toBe(shown);
    one.unmount();
    expect(revoke).not.toHaveBeenCalledWith(shown);
    two.unmount();
    expect(revoke).toHaveBeenCalledWith(shown);
  });

  it("leaves other addresses alone", () => {
    const { result } = renderHook(() => useDisplaySrc("https://example.com/a.gif"));
    expect(result.current).toBe("https://example.com/a.gif");
    expect(renderHook(() => useDisplaySrc(undefined)).result.current).toBeUndefined();
  });
});
