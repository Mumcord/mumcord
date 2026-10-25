import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { announce, LiveRegions, setHintPreference, SkipLinks } from ".";
import { createMockApi, setApi } from "../api";
import { DEFAULT_HINT_PREFERENCES, applyHintPreferences, hintPreferences } from "./hintPreferences";
import { migrateLegacyHintPreferences } from "./legacyHintPreferences";

const LEGACY_KEY = "mumcord.accessibilityHints";

afterEach(() => {
  setHintPreference("keyboardHints", true);
  setHintPreference("announcements", true);
  setHintPreference("skipLinks", true);
});

describe("hint preferences", () => {
  it("default to on", () => {
    expect(DEFAULT_HINT_PREFERENCES).toEqual({
      keyboardHints: true,
      announcements: true,
      skipLinks: true,
    });
    expect(hintPreferences()).toEqual(DEFAULT_HINT_PREFERENCES);
  });

  it("are taken over from a settings record", () => {
    applyHintPreferences({ keyboardHints: false, announcements: true, skipLinks: false });
    expect(hintPreferences()).toEqual({
      keyboardHints: false,
      announcements: true,
      skipLinks: false,
    });
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

describe("migration of the old window storage", () => {
  afterEach(() => {
    localStorage.removeItem(LEGACY_KEY);
    setApi(undefined);
  });

  it("writes stored values into the settings once and removes the key", async () => {
    const api = createMockApi();
    setApi(api);
    localStorage.setItem(LEGACY_KEY, '{"skipLinks":false,"announcements":"no"}');
    const migrated = await migrateLegacyHintPreferences(await api.settingsGet());
    expect(migrated).toMatchObject({ keyboardHints: true, announcements: true, skipLinks: false });
    expect((await api.settingsGet()).skipLinks).toBe(false);
    expect(localStorage.getItem(LEGACY_KEY)).toBeNull();
  });

  it("does nothing without old values and drops a broken entry", async () => {
    const api = createMockApi();
    setApi(api);
    const before = await api.settingsGet();
    expect(await migrateLegacyHintPreferences(before)).toEqual(before);
    localStorage.setItem(LEGACY_KEY, "not json");
    expect(await migrateLegacyHintPreferences(before)).toEqual(before);
    expect(localStorage.getItem(LEGACY_KEY)).toBeNull();
  });

  it("keeps the old values when saving fails", async () => {
    const api = createMockApi();
    setApi(api);
    const before = await api.settingsGet();
    api.settingsUpdate = () => Promise.reject(new Error("write failed"));
    localStorage.setItem(LEGACY_KEY, '{"keyboardHints":false}');
    expect(await migrateLegacyHintPreferences(before)).toEqual(before);
    expect(localStorage.getItem(LEGACY_KEY)).not.toBeNull();
  });
});
