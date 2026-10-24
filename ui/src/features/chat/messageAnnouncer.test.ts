import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMessageAnnouncer } from "./messageAnnouncer";

describe("message announcer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reads the first message and sums up a burst", () => {
    const spoken: string[] = [];
    const announcer = createMessageAnnouncer(
      (text) => spoken.push(text),
      (count) => `${String(count)} new messages`,
      1000,
    );
    announcer.push("Anna: hi");
    announcer.push("Anna: two");
    announcer.push("Anna: three");
    expect(spoken).toEqual(["Anna: hi"]);
    vi.advanceTimersByTime(1000);
    expect(spoken).toEqual(["Anna: hi", "2 new messages"]);
    vi.advanceTimersByTime(1000);
    announcer.push("Ben: later");
    expect(spoken.at(-1)).toBe("Ben: later");
    announcer.dispose();
  });
});
