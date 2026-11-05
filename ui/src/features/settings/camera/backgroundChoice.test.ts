import { describe, expect, it } from "vitest";
import { backgroundOf, choiceOf } from "./backgroundChoice";

describe("default background choice (FR-8.75)", () => {
  it("shows each stored background", () => {
    expect(choiceOf({ kind: "none" }, [])).toBe("none");
    expect(choiceOf({ kind: "blur" }, [])).toBe("blur");
    expect(choiceOf({ kind: "builtin", id: "beach" }, [])).toBe("library");
    expect(choiceOf({ kind: "own", id: "a" }, ["a"])).toBe("own:a");
  });

  it("shows Blur for an own image that is gone (FR-8.15)", () => {
    expect(choiceOf({ kind: "own", id: "gone" }, ["a"])).toBe("blur");
  });

  it("keeps the chosen library image and otherwise takes the first", () => {
    const beach = { kind: "builtin", id: "beach" } as const;
    expect(backgroundOf("library", beach, ["studio"])).toEqual(beach);
    expect(backgroundOf("library", { kind: "blur" }, ["studio", "beach"])).toEqual({
      kind: "builtin",
      id: "studio",
    });
    expect(backgroundOf("library", { kind: "blur" }, [])).toBeNull();
  });

  it("maps the other choices", () => {
    expect(backgroundOf("none", { kind: "blur" }, [])).toEqual({ kind: "none" });
    expect(backgroundOf("blur", { kind: "none" }, [])).toEqual({ kind: "blur" });
    expect(backgroundOf("own:x-1", { kind: "none" }, [])).toEqual({ kind: "own", id: "x-1" });
  });
});
