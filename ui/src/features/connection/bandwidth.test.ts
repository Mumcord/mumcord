import { describe, expect, it } from "vitest";
import { bandwidthLevel } from "./bandwidth";

describe("bandwidthLevel", () => {
  it("stays neutral below 80 % of the server limit", () => {
    expect(bandwidthLevel(0)).toBe("normal");
    expect(bandwidthLevel(0.71)).toBe("normal");
    expect(bandwidthLevel(0.799)).toBe("normal");
  });

  it("warns from 80 % up to the limit", () => {
    expect(bandwidthLevel(0.8)).toBe("warning");
    expect(bandwidthLevel(1)).toBe("warning");
  });

  it("is danger only above the limit", () => {
    expect(bandwidthLevel(1.01)).toBe("danger");
  });
});
