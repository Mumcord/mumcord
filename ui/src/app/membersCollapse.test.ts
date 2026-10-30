import { describe, expect, it } from "vitest";
import { isMembersShortcut, loadMembersCollapsed, saveMembersCollapsed } from "./membersCollapse";

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const data = new Map(Object.entries(initial));
  return {
    get length() {
      return data.size;
    },
    clear: () => {
      data.clear();
    },
    getItem: (key: string) => data.get(key) ?? null,
    key: (index: number) => [...data.keys()][index] ?? null,
    removeItem: (key: string) => {
      data.delete(key);
    },
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
  };
}

const shortcut = (overrides: Partial<Parameters<typeof isMembersShortcut>[0]>) =>
  isMembersShortcut({
    key: "m",
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ...overrides,
  });

describe("member column collapse", () => {
  it("remembers the collapsed state", () => {
    const storage = memoryStorage();
    expect(loadMembersCollapsed(storage)).toBe(false);
    saveMembersCollapsed(true, storage);
    expect(loadMembersCollapsed(storage)).toBe(true);
    saveMembersCollapsed(false, storage);
    expect(loadMembersCollapsed(storage)).toBe(false);
  });

  it("shows the column when the stored state is missing or unreadable", () => {
    expect(loadMembersCollapsed(memoryStorage())).toBe(false);
    const inner = memoryStorage();
    const broken: Storage = {
      get length() {
        return inner.length;
      },
      clear: () => {
        inner.clear();
      },
      getItem: () => {
        throw new Error("blocked");
      },
      key: (index) => inner.key(index),
      removeItem: (key) => {
        inner.removeItem(key);
      },
      setItem: (key, value) => {
        inner.setItem(key, value);
      },
    };
    expect(loadMembersCollapsed(broken)).toBe(false);
  });

  it("accepts Ctrl+Shift+M and Cmd+Shift+M only", () => {
    expect(shortcut({ ctrlKey: true, shiftKey: true })).toBe(true);
    expect(shortcut({ metaKey: true, shiftKey: true })).toBe(true);
    expect(shortcut({ key: "M", ctrlKey: true, shiftKey: true })).toBe(true);
    expect(shortcut({ ctrlKey: true })).toBe(false);
    expect(shortcut({ shiftKey: true })).toBe(false);
    expect(shortcut({ ctrlKey: true, shiftKey: true, altKey: true })).toBe(false);
    expect(shortcut({ key: "n", ctrlKey: true, shiftKey: true })).toBe(false);
  });
});
