import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, getApi, setApi, type Settings } from "../../../api";
import { chooseOption } from "../../../components/SelectTestUtils";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { encodeThemeCode, formatShareMessage } from "../../../theme/shareCodec";
import { PreferencesProvider } from "../../../app/PreferencesProvider";
import { usePreferences } from "../../../app/preferences";
import { toStoredTheme } from "../../../theme/appTheme";
import { generateVersion } from "../../../theme/versions";
import { findPreset, MUMCORD_PRESET } from "../../../theme/themeModel";
import { getThemeChoice, resetThemeChoice } from "../../../theme/themeStore";
import { ThemeEditor } from "./ThemeEditor";

// What the editor saved last; written by the update callback, not during render.
const saved: { settings: Settings | null } = { settings: null };

const DARK = MUMCORD_PRESET.dark;
const LIGHT = MUMCORD_PRESET.light;

/** Holds settings in state and applies them, like the settings screen does. */
function Editor({ initial }: { initial: Settings }) {
  const [settings, setSettings] = useState(initial);
  const { applySettings } = usePreferences();
  return (
    <main aria-label="Theme test">
      <ThemeEditor
        settings={settings}
        update={(patch) => {
          const next = { ...(saved.settings ?? initial), ...patch };
          saved.settings = next;
          setSettings(next);
          applySettings(next);
        }}
      />
    </main>
  );
}

function Harness({ initial }: { initial: Settings }) {
  return (
    <PreferencesProvider>
      <Editor initial={initial} />
    </PreferencesProvider>
  );
}

function latestSettings(): Settings {
  if (!saved.settings) throw new Error("nothing saved yet");
  return saved.settings;
}

async function renderEditor(patch: Partial<Settings> = {}) {
  const api = createMockApi();
  setApi(api);
  const initial = { ...(await api.settingsGet()), ...patch };
  saved.settings = initial;
  const view = render(<Harness initial={initial} />);
  // The version for Share and Import arrives from the backend.
  await waitFor(() => {
    expect(screen.getByRole("button", { name: "Import…" })).toBeEnabled();
  });
  return view;
}

const modeSelect = () => screen.getByRole("combobox", { name: "Dark or light" });
const circle = (name: string) =>
  screen.getByRole("button", { name: new RegExp(`^${name}, theme(, selected)?$`) });
const selectedCircle = () => screen.getByRole("button", { name: /, theme, selected$/ });
const cell = (color: string, version: "Light" | "Dark") =>
  screen.getByRole("textbox", { name: `${color}, ${version}` });
const addCircle = () => screen.getByRole("button", { name: "New theme from the current one" });
const editor = () => screen.getByRole("dialog", { name: /^(New theme|Edit “.*”)$/ });

/** A hex colour as the browser writes it back in an inline style. */
function rgb(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));
  return `rgb(${String(r)}, ${String(g)}, ${String(b)})`;
}

function openMenu(name: string) {
  fireEvent.contextMenu(circle(name), { clientX: 10, clientY: 10 });
  return screen.getByRole("menu", { name: `Options for ${name}` });
}

function typeHex(color: string, version: "Light" | "Dark", value: string) {
  fireEvent.change(cell(color, version), { target: { value } });
}

const mine = toStoredTheme({
  id: "custom:a",
  name: "Mine",
  dark: DARK,
  light: generateVersion(DARK, "light"),
  generated: "light",
});

beforeEach(async () => {
  await applyLanguage("en");
  resetThemeChoice();
});

afterEach(() => {
  vi.unstubAllGlobals();
  setApi(undefined);
});

describe("ThemeEditor (FR-2.14 – FR-2.16, FR-2.63)", () => {
  it("shows the presets and Your themes as circles, and a click applies a theme", async () => {
    const { container } = await renderEditor({ customThemes: [mine] });
    expect(
      screen
        .getAllByRole("button", { name: /, theme(, selected)?$/ })
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Mumcord, theme, selected",
      "Deep Ocean, theme",
      "Forest, theme",
      "Paper, theme",
      "Mine, theme",
    ]);
    expect(
      within(screen.getByRole("list", { name: "Your themes" })).getAllByRole("button"),
    ).toEqual([circle("Mine"), addCircle()]);
    fireEvent.click(circle("Paper"));
    expect(latestSettings().appThemeId).toBe("preset:paper");
    expect(latestSettings().theme).toBe("system");
    expect(selectedCircle()).toBe(circle("Paper"));
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
    expect(await axeViolations(container)).toEqual([]);
  });

  it("shows the circles in the version the Dark / Light setting picks", async () => {
    await renderEditor({ appThemeId: "preset:ocean" });
    const ocean = findPreset("preset:ocean") ?? MUMCORD_PRESET;
    expect(circle("Deep Ocean").style.background).toContain(rgb(ocean.dark.background));
    chooseOption(modeSelect(), "Light");
    expect(latestSettings().theme).toBe("light");
    await waitFor(() => {
      expect(circle("Deep Ocean").style.background).toContain(rgb(ocean.light.background));
    });
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "light" });
  });

  it("creates a theme with + and Save as theme, editing Light without touching Dark", async () => {
    await renderEditor();
    fireEvent.click(addCircle());
    expect(screen.queryByRole("button", { name: "Dark", pressed: true })).toBeNull();
    const table = within(editor()).getByRole("table", { name: "Key colours" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((th) => th.textContent),
    ).toEqual(["Colour", "Light", "Dark"]);
    typeHex("Accent", "Light", "#5020c0");
    expect(cell("Accent", "Dark")).toHaveValue(DARK.accent);
    expect(cell("Background", "Dark")).toHaveValue(DARK.background);

    fireEvent.change(screen.getByRole("textbox", { name: "Theme name" }), {
      target: { value: "Night" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save as theme" }));
    const [night] = latestSettings().customThemes;
    expect(night?.name).toBe("Night");
    expect(night?.colorsLight).toEqual({ ...LIGHT, accent: "#5020c0" });
    expect(night?.colorsDark).toEqual(DARK);
    expect(latestSettings().appThemeId).toBe(night?.id);
    expect(selectedCircle()).toBe(circle("Night"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Theme saved.");
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
  });

  it("labels every colour of both versions for keyboard and screen-reader use", async () => {
    await renderEditor();
    fireEvent.click(addCircle());
    for (const name of ["Background", "Surface", "Text", "Accent", "Talking and online"]) {
      for (const version of ["Light", "Dark"] as const) {
        expect(cell(name, version)).toBeInTheDocument();
        expect(screen.getByLabelText(`${name}, ${version} colour picker`)).toHaveAttribute(
          "type",
          "color",
        );
      }
    }
  });

  it("offers New theme from this and Share for a preset, which saves a new own theme", async () => {
    await renderEditor({ appThemeId: "preset:forest" });
    const menu = openMenu("Deep Ocean");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["New theme from this", "Share"]);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "New theme from this" }));
    expect(editor()).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Theme name" })).toHaveValue("Deep Ocean (copy)");
    typeHex("Accent", "Dark", "#ff66cc");

    fireEvent.click(screen.getByRole("button", { name: "Save as theme" }));
    const ocean = findPreset("preset:ocean") ?? MUMCORD_PRESET;
    const [copy] = latestSettings().customThemes;
    expect(copy?.name).toBe("Deep Ocean (copy)");
    expect(copy?.colorsDark).toEqual({ ...ocean.dark, accent: "#ff66cc" });
    expect(copy?.colorsLight).toEqual(ocean.light);
    expect(latestSettings().appThemeId).toBe(copy?.id);
  });

  it("warns in the colour's row and blocks saving an unreadable theme", async () => {
    await renderEditor();
    fireEvent.click(addCircle());
    typeHex("Text", "Dark", "#222222");
    const warning = screen.getByText(/Dark: contrast too low/);
    expect(screen.getByRole("rowheader", { name: /^Text/ })).toContainElement(warning);
    expect(cell("Text", "Dark")).toHaveAccessibleDescription(warning.textContent);
    expect(cell("Text", "Light")).not.toHaveAccessibleDescription(warning.textContent);
    expect(screen.getByRole("alert")).toHaveTextContent("isn't readable enough");
    expect(screen.getByRole("button", { name: "Save as theme" })).toBeDisabled();
  });

  it("keeps a half-typed hex value out of the theme and Reset discards the draft", async () => {
    await renderEditor();
    fireEvent.click(addCircle());
    typeHex("Background", "Dark", "#12");
    expect(cell("Background", "Dark")).toHaveAttribute("aria-invalid", "true");

    typeHex("Background", "Dark", "#101010");
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(cell("Background", "Dark")).toHaveValue(DARK.background);
    fireEvent.click(within(editor()).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(latestSettings().customThemes).toEqual([]);
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
  });

  it("marks the generated version auto and keeps it in step until it is edited", async () => {
    await renderEditor({ customThemes: [mine], appThemeId: "custom:a" });
    fireEvent.click(within(openMenu("Mine")).getByRole("menuitem", { name: "Edit theme" }));
    const header = (name: string) =>
      screen.getByRole("columnheader", { name: new RegExp(`^${name}`) });
    expect(header("Light")).toHaveTextContent("Lightauto");
    expect(header("Dark")).toHaveTextContent(/^Dark$/);
    expect(screen.getByText(/Made automatically from the dark version/)).toBeInTheDocument();

    typeHex("Accent", "Dark", "#ff66cc");
    expect(cell("Accent", "Light")).toHaveValue(
      generateVersion({ ...DARK, accent: "#ff66cc" }, "light").accent,
    );

    typeHex("Accent", "Light", "#5020c0");
    expect(header("Light")).toHaveTextContent(/^Light$/);
    expect(screen.queryByText(/Made automatically/)).toBeNull();
    typeHex("Accent", "Dark", "#ffaa00");
    expect(cell("Accent", "Light")).toHaveValue("#5020c0");

    fireEvent.click(screen.getByRole("button", { name: "Save theme" }));
    const [saved] = latestSettings().customThemes;
    expect(saved?.colorsDark.accent).toBe("#ffaa00");
    expect(saved?.colorsLight.accent).toBe("#5020c0");
    expect(saved?.lightGenerated).toBe(false);
    expect(saved?.darkGenerated).toBe(false);
  });

  it("checks contrast on both versions before saving", async () => {
    await renderEditor({ customThemes: [mine], appThemeId: "custom:a" });
    fireEvent.click(within(openMenu("Mine")).getByRole("menuitem", { name: "Edit theme" }));
    typeHex("Text", "Light", "#eeeeee");
    expect(screen.getByRole("alert")).toHaveTextContent("isn't readable enough");
    expect(screen.getByRole("button", { name: "Save theme" })).toBeDisabled();
  });

  it("renames and deletes an own theme from its context menu", async () => {
    await renderEditor({ customThemes: [mine], appThemeId: "custom:a" });
    expect(
      within(openMenu("Mine"))
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Edit theme", "New theme from this", "Share", "Rename theme", "Delete theme"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Rename theme" }));
    const dialog = screen.getByRole("dialog", { name: "Rename “Mine”" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Theme name" }), {
      target: { value: "  Mine 2 " },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Rename" }));
    expect(latestSettings().customThemes).toEqual([{ ...mine, name: "Mine 2" }]);
    expect(selectedCircle()).toBe(circle("Mine 2"));

    fireEvent.click(within(openMenu("Mine 2")).getByRole("menuitem", { name: "Delete theme" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    expect(latestSettings().customThemes).toEqual([]);
    expect(latestSettings().appThemeId).toBe("preset:mumcord");
    expect(screen.queryByRole("button", { name: /^Mine 2, theme/ })).toBeNull();
    expect(selectedCircle()).toBe(circle("Mumcord"));
  });

  it("copies the share message with the app version", async () => {
    await renderEditor({ appThemeId: "preset:forest" });
    const writeText = vi.spyOn(getApi(), "copyText").mockResolvedValue(null);
    fireEvent.click(within(openMenu("Forest")).getByRole("menuitem", { name: "Share" }));
    const forest = findPreset("preset:forest");
    if (!forest) throw new Error("no preset");
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(formatShareMessage(forest, "0.0.0-mock"));
    });
    expect(screen.getByRole("status")).toHaveTextContent("Theme copied");
  });

  it("opens the editor in a dialog and Cancel or Escape reverts the preview", async () => {
    await renderEditor({ customThemes: [mine], appThemeId: "custom:a" });
    fireEvent.click(addCircle());
    expect(editor()).toHaveAttribute("aria-modal", "true");
    typeHex("Accent", "Dark", "#ff66cc");
    fireEvent.click(within(editor()).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
    expect(latestSettings().customThemes).toEqual([mine]);

    fireEvent.click(within(openMenu("Mine")).getByRole("menuitem", { name: "Edit theme" }));
    expect(within(editor()).getByRole("button", { name: "Save theme" })).toBeInTheDocument();
    fireEvent.keyDown(editor(), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
  });

  it("previews the draft in the whole app, keeps edits, and Escape returns to the dialog", async () => {
    await renderEditor();
    fireEvent.click(addCircle());
    typeHex("Accent", "Dark", "#ff66cc");
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });

    fireEvent.click(within(editor()).getByRole("button", { name: "Preview" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("region", { name: "Previewing Mumcord (copy)" })).toBeInTheDocument();
    const preview = getThemeChoice().preview;
    expect(preview?.kind === "draft" && preview.theme.dark.accent).toBe("#ff66cc");
    expect(preview?.kind === "draft" && preview.variant).toBe("dark");

    fireEvent.click(screen.getByRole("button", { name: "Back to editor" }));
    expect(screen.queryByRole("region", { name: /^Previewing/ })).toBeNull();
    expect(cell("Accent", "Dark")).toHaveValue("#ff66cc");
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });

    fireEvent.click(within(editor()).getByRole("button", { name: "Preview" }));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(editor()).toBeInTheDocument();
    expect(cell("Accent", "Dark")).toHaveValue("#ff66cc");

    fireEvent.click(within(editor()).getByRole("button", { name: "Cancel" }));
    expect(latestSettings().customThemes).toEqual([]);
    expect(latestSettings().appThemeId).toBe("mumcord-dark");
    expect(getThemeChoice().preview).toEqual({ kind: "app", variant: "dark" });
  });

  it("starts a new theme from an own theme's menu", async () => {
    await renderEditor({ customThemes: [mine], appThemeId: "custom:a" });
    fireEvent.click(
      within(openMenu("Mine")).getByRole("menuitem", { name: "New theme from this" }),
    );
    expect(screen.getByRole("textbox", { name: "Theme name" })).toHaveValue("Mine (copy)");
    expect(within(editor()).getByRole("button", { name: "Save as theme" })).toBeInTheDocument();
  });

  describe("Import", () => {
    const shared = { name: "Sunset", dark: { ...DARK, accent: "#ff8844" }, light: LIGHT };

    function importText(text: string) {
      fireEvent.click(screen.getByRole("button", { name: "Import…" }));
      const dialog = screen.getByRole("dialog", { name: "Import a theme" });
      fireEvent.change(within(dialog).getByRole("textbox", { name: "Theme message or code" }), {
        target: { value: text },
      });
      fireEvent.click(within(dialog).getByRole("button", { name: "Import" }));
      return dialog;
    }

    it("explains malformed and unreadable codes in plain language", async () => {
      await renderEditor();
      const dialog = importText("hello mct1.@@@ not a theme");
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "This isn't a valid Mumcord theme.",
      );
      expect(within(dialog).getByRole("textbox")).toHaveAttribute("aria-invalid", "true");

      const unreadable = encodeThemeCode(
        { name: "Grey", dark: { ...DARK, text: DARK.background }, light: LIGHT },
        "0.0.0",
      );
      fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: unreadable } });
      fireEvent.click(within(dialog).getByRole("button", { name: "Import" }));
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "This theme isn't readable enough.",
      );
      expect(latestSettings().customThemes).toEqual([]);
    });

    it("imports a shared message once and selects it", async () => {
      await renderEditor();
      importText(`look: ${formatShareMessage(shared, "0.0.0")}`);
      expect(latestSettings().customThemes).toHaveLength(1);
      expect(latestSettings().appThemeId).toBe(latestSettings().customThemes[0]?.id);
      expect(screen.getByRole("status")).toHaveTextContent("Imported “Sunset”.");

      importText(formatShareMessage(shared, "0.0.0"));
      expect(latestSettings().customThemes).toHaveLength(1);
      expect(screen.getByRole("status")).toHaveTextContent("already in your themes");
    });

    it("notes a theme made with a newer Mumcord", async () => {
      await renderEditor();
      importText(formatShareMessage(shared, "9.0.0"));
      expect(screen.getByRole("status")).toHaveTextContent(
        "Made with Mumcord 9.0.0 – update to see it fully.",
      );
    });
  });
});
