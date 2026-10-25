import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { announce, LiveRegions, setHintPreference, SkipLinks } from ".";
import { DEFAULT_HINT_PREFERENCES, loadHintPreferences } from "./hintPreferences";

function memoryStorage(value: string | null): Storage {
  return { getItem: () => value } as unknown as Storage;
}

afterEach(() => {
  setHintPreference("keyboardHints", true);
  setHintPreference("announcements", true);
  setHintPreference("skipLinks", true);
});

describe("hint preferences", () => {
  it("default to on and ignore broken or partial records", () => {
    expect(DEFAULT_HINT_PREFERENCES).toEqual({
      keyboardHints: true,
      announcements: true,
      skipLinks: true,
    });
    expect(loadHintPreferences(memoryStorage(null))).toEqual(DEFAULT_HINT_PREFERENCES);
    expect(loadHintPreferences(memoryStorage("not json"))).toEqual(DEFAULT_HINT_PREFERENCES);
    expect(loadHintPreferences(memoryStorage('{"skipLinks":false,"announcements":"no"}'))).toEqual({
      keyboardHints: true,
      announcements: true,
      skipLinks: false,
    });
  });

  it("are saved in the window's storage", () => {
    setHintPreference("announcements", false);
    expect(loadHintPreferences()).toEqual({ ...DEFAULT_HINT_PREFERENCES, announcements: false });
  });

  it("keep the announcer silent while announcements are off", () => {
    render(<LiveRegions />);
    act(() => {
      setHintPreference("announcements", false);
      announce("Anna joined");
    });
    expect(screen.getByTestId("live-polite")).toHaveTextContent("");
    act(() => {
      setHintPreference("announcements", true);
      announce("Ben joined");
    });
    expect(screen.getByTestId("live-polite")).toHaveTextContent("Ben joined");
  });

  it("remove the skip links while they are off", () => {
    render(<SkipLinks targets={[{ id: "chat", label: "Skip to chat" }]} />);
    expect(screen.getByRole("link", { name: "Skip to chat" })).toBeInTheDocument();
    act(() => {
      setHintPreference("skipLinks", false);
    });
    expect(screen.queryByRole("link", { name: "Skip to chat" })).toBeNull();
  });
});
