/** Small figures of the screen share views (issue #163). */
import { describe, expect, it } from "vitest";
import { litBars, qualityText } from "./shareFormat";

describe("share figures", () => {
  it("writes a viewer's quality as height and frame rate", () => {
    expect(qualityText(1080, 60)).toBe("1080p · 60 fps");
  });

  it("lights the sound bars with the level, none without sound", () => {
    expect(litBars(null)).toBe(0);
    expect(litBars(0)).toBe(0);
    expect(litBars(0.42)).toBe(3);
    expect(litBars(1.5)).toBe(5);
  });
});
