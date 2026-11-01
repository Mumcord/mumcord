import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, getApi, setApi, type Settings } from "../api";
import { applyLanguage } from "../i18n";
import { axeViolations } from "../test/axe";
import { App } from "./App";
import { stubDialog } from "../features/channels/testUtils";
import { usePreferences } from "./preferences";
import { useView } from "./view";

// A user panel stand-in that exposes the view and preference contracts to the test.
vi.mock("./mounts", async (importOriginal) => {
  const original = await importOriginal<typeof import("./mounts")>();
  function UserPanelMount() {
    const { openSettings } = useView();
    const { applySettings } = usePreferences();
    return (
      <>
        <button
          type="button"
          onClick={() => {
            openSettings();
          }}
        >
          Open settings
        </button>
        <button
          type="button"
          onClick={() => {
            void getApi()
              .settingsUpdate({ ...DEFAULTS, theme: "light", language: "es" })
              .then(applySettings);
          }}
        >
          Light Spanish
        </button>
      </>
    );
  }
  return { ...original, UserPanelMount };
});

let DEFAULTS: Settings;

async function renderWith(seed: "fresh" | "ready", overrides: Partial<Settings> = {}) {
  const api = createMockApi({ seed });
  DEFAULTS = await api.settingsGet();
  await api.settingsUpdate({ ...DEFAULTS, updateCheck: "no", ...overrides });
  setApi(api);
  return render(<App />);
}

beforeEach(stubDialog);

afterEach(async () => {
  setApi(undefined);
  await applyLanguage("en");
});

describe("App", () => {
  it("shows the wizard on first start", async () => {
    await renderWith("fresh");
    expect(await screen.findByRole("heading", { name: "Welcome to Mumcord" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Servers" })).not.toBeInTheDocument();
  });

  it("shows the four-column shell with all regions when set up", async () => {
    await renderWith("ready");
    expect(await screen.findByRole("navigation", { name: "Servers" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Channels" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Your status" })).toBeInTheDocument();
    expect(screen.getByRole("main", { name: "Chat" })).toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Members" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Skip to chat" })).toHaveAttribute("href", "#chat");
  });

  it.each(["dark", "light"] as const)("has no axe violations in the %s theme", async (theme) => {
    const { container } = await renderWith("ready", { theme });
    await screen.findByRole("main", { name: "Chat" });
    expect(document.documentElement.dataset.theme).toBe(theme);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("uses the stored language setting", async () => {
    await renderWith("ready", { language: "de" });
    expect(await screen.findByRole("navigation", { name: "Kanäle" })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("de");
  });

  it("applies theme and language changes without restart", async () => {
    await renderWith("ready", { theme: "dark" });
    fireEvent.click(await screen.findByRole("button", { name: "Light Spanish" }));
    expect(await screen.findByRole("navigation", { name: "Canales" })).toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("opens settings as a modal, closes on Escape and restores focus", async () => {
    await renderWith("ready");
    const opener = await screen.findByRole("button", { name: "Open settings" });
    opener.focus();
    fireEvent.click(opener);
    const dialog = screen.getByRole("dialog", { name: "Settings" });
    expect(dialog).toHaveFocus();
    expect(
      screen.getByRole("main", { name: "Chat", hidden: true }).closest("[inert]"),
    ).not.toBeNull();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
  });

  it("hides Settings during a theme preview and restores it with the draft", async () => {
    await renderWith("ready");
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    fireEvent.click(await screen.findByRole("tab", { name: "Appearance" }));
    fireEvent.click(await screen.findByRole("button", { name: "New theme from the current one" }));
    const editor = screen.getByRole("dialog", { name: /^New theme/ });
    const accent = within(editor).getByRole("textbox", { name: "Accent, Dark" });
    fireEvent.change(accent, { target: { value: "#ff66cc" } });
    fireEvent.click(within(editor).getByRole("button", { name: "Preview" }));

    const back = screen.getByRole("button", { name: "Back to editor" });
    expect(back).toHaveFocus();
    expect(screen.queryByRole("dialog", { name: "Settings" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Settings", hidden: true }).closest(".overlay"),
    ).toHaveAttribute("hidden");
    expect(screen.getByRole("main", { name: "Chat" }).closest("[inert]")).toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.getByRole("dialog", { name: "Settings" })).toBeInTheDocument();
    const restored = screen.getByRole("dialog", { name: /^New theme/ });
    expect(within(restored).getByRole("textbox", { name: "Accent, Dark" })).toHaveValue("#ff66cc");
    expect(
      screen.getByRole("main", { name: "Chat", hidden: true }).closest("[inert]"),
    ).not.toBeNull();
  });

  it("shows a retryable error when settings cannot be loaded", async () => {
    const api = createMockApi({ seed: "ready" });
    api.mock.failNext("settingsGet", "secureStorageUnavailable");
    setApi(api);
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<App />);
    const retry = await screen.findByRole("button", { name: "Try again" });
    expect(consoleError).toHaveBeenCalled();
    fireEvent.click(retry);
    await waitFor(() => {
      expect(screen.getByRole("main", { name: "Chat" })).toBeInTheDocument();
    });
    consoleError.mockRestore();
  });
});
