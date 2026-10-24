import { describe, expect, it, vi } from "vitest";
import {
  clampWidth,
  DEFAULT_COLUMN_WIDTHS,
  fitWidths,
  loadColumnWidths,
  maxWidth,
  saveColumnWidths,
  widthForDrag,
  widthForKey,
  type ShellRoom,
} from "./columnWidths";

/** A window `shell` rem wide with the 4.5rem server bar; the chat keeps 22.5rem. */
const room = (shell: number, membersShown = true): ShellRoom => ({
  shell,
  serverBar: 4.5,
  membersShown,
});

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const data = new Map(Object.entries(initial));
  return {
    get length() {
      return data.size;
    },
    clear: () => {
      data.clear();
    },
    getItem: (key) => data.get(key) ?? null,
    key: (index) => [...data.keys()][index] ?? null,
    removeItem: (key) => {
      data.delete(key);
    },
    setItem: (key, value) => {
      data.set(key, value);
    },
  };
}

describe("clampWidth", () => {
  it("keeps widths between 15rem and 28rem", () => {
    expect([10, 15, 20.5, 28, 40].map((w) => clampWidth(w))).toEqual([15, 15, 20.5, 28, 28]);
  });

  it("never goes below the minimum, even when less room is left", () => {
    expect(clampWidth(20, 18)).toBe(18);
    expect(clampWidth(20, 10)).toBe(15);
  });
});

describe("fitWidths", () => {
  it("keeps the chosen widths when the window has room", () => {
    expect(fitWidths({ channels: 24, members: 20 }, room(90))).toEqual({
      channels: 24,
      members: 20,
    });
  });

  it("shrinks the member list first so the chat keeps its minimum", () => {
    // 70 - 4.5 - 22.5 = 43rem for both columns.
    expect(fitWidths({ channels: 24, members: 24 }, room(70))).toEqual({
      channels: 24,
      members: 19,
    });
    expect(fitWidths({ channels: 28, members: 28 }, room(60))).toEqual({
      channels: 18,
      members: 15,
    });
  });

  it("stops at the minimum widths on very small windows", () => {
    expect(fitWidths({ channels: 28, members: 28 }, room(40))).toEqual(DEFAULT_COLUMN_WIDTHS);
  });

  it("ignores the member list when it is an overlay on narrow windows", () => {
    expect(fitWidths({ channels: 28, members: 20 }, room(50, false))).toEqual({
      channels: 23,
      members: 20,
    });
  });
});

describe("maxWidth", () => {
  it("is what the chat and the other column leave, within the limits", () => {
    const shown = { channels: 20, members: 18 };
    expect(maxWidth("channels", shown, room(70))).toBe(25);
    expect(maxWidth("members", shown, room(70))).toBe(23);
    expect(maxWidth("channels", shown, room(120))).toBe(28);
    expect(maxWidth("channels", shown, room(50, false))).toBe(23);
  });
});

describe("widthForKey", () => {
  it("moves the channel list border with the arrow keys by 1rem", () => {
    expect(widthForKey("ArrowRight", "channels", 15, 28)).toBe(16);
    expect(widthForKey("ArrowLeft", "channels", 18, 28)).toBe(17);
    expect(widthForKey("ArrowLeft", "channels", 15, 28)).toBe(15);
    expect(widthForKey("ArrowRight", "channels", 20, 20)).toBe(20);
  });

  it("widens the member list with the left arrow, since its border is on the left", () => {
    expect(widthForKey("ArrowLeft", "members", 15, 28)).toBe(16);
    expect(widthForKey("ArrowRight", "members", 16, 28)).toBe(15);
  });

  it("jumps to the limits with Home and End and ignores other keys", () => {
    expect(widthForKey("Home", "channels", 22, 28)).toBe(15);
    expect(widthForKey("End", "members", 15, 28)).toBe(28);
    expect(widthForKey("End", "channels", 15, 21)).toBe(21);
    expect(widthForKey("Enter", "channels", 15, 28)).toBeNull();
  });
});

describe("widthForDrag", () => {
  it("converts pointer movement to rem in the column's direction", () => {
    expect(widthForDrag("channels", 15, 48, 16, 28)).toBe(18);
    expect(widthForDrag("members", 15, -48, 16, 28)).toBe(18);
    expect(widthForDrag("channels", 15, 48, 32, 28)).toBe(16.5);
  });

  it("clamps to the limits", () => {
    expect(widthForDrag("channels", 15, -200, 16, 28)).toBe(15);
    expect(widthForDrag("channels", 15, 1000, 16, 24)).toBe(24);
  });
});

describe("persistence", () => {
  it("remembers the widths", () => {
    const storage = memoryStorage();
    saveColumnWidths({ channels: 22, members: 17.5 }, storage);
    expect(loadColumnWidths(storage)).toEqual({ channels: 22, members: 17.5 });
  });

  it("falls back to the defaults for missing, broken or out-of-range values", () => {
    expect(loadColumnWidths(memoryStorage())).toEqual(DEFAULT_COLUMN_WIDTHS);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    expect(loadColumnWidths(memoryStorage({ "mumcord.columnWidths": "{" }))).toEqual(
      DEFAULT_COLUMN_WIDTHS,
    );
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
    expect(
      loadColumnWidths(memoryStorage({ "mumcord.columnWidths": '{"channels":99,"members":"x"}' })),
    ).toEqual({ channels: 28, members: 15 });
  });

  it("survives storage that throws", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const broken = memoryStorage();
    broken.getItem = () => {
      throw new Error("denied");
    };
    broken.setItem = () => {
      throw new Error("quota");
    };
    expect(loadColumnWidths(broken)).toEqual(DEFAULT_COLUMN_WIDTHS);
    expect(() => {
      saveColumnWidths(DEFAULT_COLUMN_WIDTHS, broken);
    }).not.toThrow();
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });
});
