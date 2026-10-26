import { act, render, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../api";
import { resetServerStore, showServer } from "../features/channels/serverStore";
import { THEME_TOKENS } from "../theme/derive";
import { applyThemeToDocument, clearThemeColors } from "../theme/theme";
import { findPreset } from "../theme/themeModel";
import { resetThemeChoice } from "../theme/themeStore";
import { usePreferences } from "./preferences";
import { PreferencesProvider } from "./PreferencesProvider";
import { useServerTheme } from "./useServerTheme";

let api: MockApi;

function Harness() {
  const { applySettings } = usePreferences();
  useServerTheme();
  useEffect(() => {
    void api.settingsGet().then(applySettings);
  }, [applySettings]);
  return null;
}

const firstToken = `--color-${THEME_TOKENS[0]}`;
const inlineColor = () => document.documentElement.style.getPropertyValue(firstToken);

beforeEach(async () => {
  resetServerStore();
  resetThemeChoice();
  clearThemeColors();
  applyThemeToDocument("dark");
  api = createMockApi();
  setApi(api);
  const other = await api.serverAdd({
    label: "Other",
    address: "other.example.org",
    port: 64738,
    username: "You",
    password: { kind: "keep" },
  });
  await api.serverSetTheme(other.id, "preset:paper");
});

afterEach(() => {
  setApi(undefined);
});

describe("per-server theme (FR-2.17, AC-2.14)", () => {
  it("shows the server's theme while it is shown and the app theme for others", async () => {
    const [first, second] = await api.serversList();
    if (!first || !second) throw new Error("two servers expected");
    render(
      <PreferencesProvider>
        <Harness />
      </PreferencesProvider>,
    );

    act(() => {
      showServer(second.id);
    });
    // Follow system without OS information shows the dark version of the server's theme.
    const paper = findPreset("preset:paper");
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--color-bg-chat")).toBe(
        paper?.dark.background,
      );
    });
    expect(document.documentElement.dataset.theme).toBe("dark");

    act(() => {
      showServer(first.id);
    });
    await waitFor(() => {
      expect(inlineColor()).toBe("");
    });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});
