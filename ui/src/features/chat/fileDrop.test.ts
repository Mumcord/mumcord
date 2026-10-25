import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, EngineError, setApi } from "../../api";
import { isOverRect, readDroppedFiles } from "./fileDrop";

afterEach(() => {
  setApi(undefined);
});

describe("isOverRect", () => {
  const rect = new DOMRect(100, 500, 400, 80);

  it("converts physical drop positions to CSS pixels", () => {
    expect(isOverRect({ x: 300, y: 1100 }, rect, 2)).toBe(true);
    // The same physical point at scale 1 is far below the composer.
    expect(isOverRect({ x: 300, y: 1100 }, rect, 1)).toBe(false);
    expect(isOverRect({ x: 150, y: 520 }, rect, 1)).toBe(true);
  });

  it("is false outside the box and for hidden elements", () => {
    expect(isOverRect({ x: 99, y: 520 }, rect, 1)).toBe(false);
    expect(isOverRect({ x: 150, y: 581 }, rect, 1)).toBe(false);
    expect(isOverRect({ x: 0, y: 0 }, new DOMRect(0, 0, 0, 0), 1)).toBe(false);
  });
});

describe("readDroppedFiles", () => {
  it("turns dropped images into files and reports the ones that failed", async () => {
    const api = createMockApi();
    vi.spyOn(api, "readDroppedImage").mockImplementation((path) => {
      if (path.endsWith("a.png")) {
        return Promise.resolve({ name: "a.png", mime: "image/png", data: btoa("PNG!") });
      }
      if (path.endsWith("big.png")) {
        return Promise.reject(new EngineError("messageTooLarge", "too large"));
      }
      return Promise.reject(new EngineError("invalidInput", "not an image"));
    });
    setApi(api);
    const { files, failures } = await readDroppedFiles([
      "/home/me/a.png",
      "C:\\Users\\me\\big.png",
      "/home/me/notes.txt",
    ]);
    expect(files).toHaveLength(1);
    expect(files[0]?.name).toBe("a.png");
    expect(files[0]?.type).toBe("image/png");
    expect(await files[0]?.text()).toBe("PNG!");
    expect(failures).toEqual([
      { name: "big.png", reason: "sourceTooLarge" },
      { name: "notes.txt", reason: "notImage" },
    ]);
  });
});
