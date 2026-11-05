import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, EngineError, setApi, type MockApi, type SoundApp } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { stubDialog } from "../channels/testUtils";
import { SoundAppPicker } from "./SoundAppPicker";

const FF_ICON = "data:image/svg+xml;base64,PHN2Zy8+";
const APPS: SoundApp[] = [
  { id: "ff", name: "Firefox", icon: FF_ICON },
  { id: "sp", name: "Spotify", icon: null },
];

let api: MockApi;

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.useRealTimers();
});

function open(apps: SoundApp[] = APPS) {
  const choose = vi.spyOn(api, "screenShareChooseSound").mockResolvedValue(null);
  const view = render(<SoundAppPicker serverId={1} apps={apps} />);
  const dialog = screen.getByRole("dialog", { name: "Which app's sound?", hidden: true });
  const list = within(dialog).getByRole("listbox", { hidden: true });
  return { choose, dialog, list, view };
}

const names = (list: HTMLElement) =>
  within(list)
    .getAllByRole("option", { hidden: true })
    .map((o) => o.textContent);

describe("SoundAppPicker", () => {
  it("lists the apps with No sound last and the first app chosen", async () => {
    const { dialog, list } = open();
    expect(names(list)).toEqual(["Firefox", "Spotify", "No sound"]);
    expect(within(list).getByRole("option", { name: "Firefox", hidden: true })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("shows the app's own icon and a coloured square without one", () => {
    const { list } = open();
    const firefox = within(list).getByRole("option", { name: "Firefox", hidden: true });
    const icon = firefox.querySelector("img");
    expect(icon).toHaveAttribute("src", FF_ICON);
    // Decorative: the row is named by the app's name only.
    expect(icon).toHaveAttribute("alt", "");
    const spotify = within(list).getByRole("option", { name: "Spotify", hidden: true });
    expect(spotify.querySelector("img")).toBeNull();
    const square = spotify.querySelector<HTMLElement>(".sound-picker__icon");
    expect(square).toHaveAttribute("aria-hidden", "true");
    // One of the mockup's square colours, not a generated one.
    expect(["#e66000", "#1db954", "#ff8800", "#e53935"]).toContain(
      square?.style.getPropertyValue("--app-colour"),
    );
  });

  it("shares the chosen app's sound", () => {
    const { choose, list, dialog } = open();
    fireEvent.click(within(list).getByRole("option", { name: "Spotify", hidden: true }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Share sound", hidden: true }));
    expect(choose).toHaveBeenCalledWith(1, "sp");
  });

  it("answers with no app for No sound", () => {
    const { choose, list, dialog } = open();
    fireEvent.click(within(list).getByRole("option", { name: "No sound", hidden: true }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Share sound", hidden: true }));
    expect(choose).toHaveBeenCalledWith(1, null);
  });

  it("answers with no app for Cancel", () => {
    const { choose, dialog } = open();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel", hidden: true }));
    expect(choose).toHaveBeenCalledWith(1, null);
  });

  it("moves with the arrow keys, Home and End, and Enter shares", () => {
    const { choose, list } = open();
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(list).toHaveAttribute("aria-activedescendant", expect.stringContaining("sp"));
    fireEvent.keyDown(list, { key: "End" });
    expect(list).toHaveAttribute("aria-activedescendant", expect.stringContaining("none"));
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Home" });
    fireEvent.keyDown(list, { key: "Enter" });
    expect(choose).toHaveBeenCalledWith(1, "ff");
  });

  it("Escape cancels", () => {
    const { choose, dialog } = open();
    fireEvent(dialog, new Event("cancel", { cancelable: true }));
    expect(choose).toHaveBeenCalledWith(1, null);
  });

  it("shows the empty state, keeps No sound and disables Share sound until chosen", async () => {
    const { dialog, list } = open([]);
    expect(names(list)).toEqual(["No sound"]);
    expect(within(dialog).getByRole("status", { hidden: true })).toHaveTextContent(
      "No app is playing sound right now",
    );
    const share = within(dialog).getByRole("button", { name: "Share sound", hidden: true });
    expect(share).toBeDisabled();
    fireEvent.click(within(list).getByRole("option", { name: "No sound", hidden: true }));
    expect(share).toBeEnabled();
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("refreshes the list while open and drops a chosen app that stopped", async () => {
    vi.useFakeTimers();
    const { list, dialog } = open();
    vi.spyOn(api, "screenShareSoundApps").mockResolvedValue([
      { id: "sp", name: "Spotify", icon: null },
    ]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2100);
    });
    expect(names(list)).toEqual(["Spotify", "No sound"]);
    expect(
      within(dialog).getByRole("button", { name: "Share sound", hidden: true }),
    ).toBeDisabled();
  });

  it("keeps the dialog open and refreshes when the chosen app stopped", async () => {
    const { choose, list, dialog } = open();
    choose.mockRejectedValueOnce(new EngineError("notFound", "gone"));
    vi.spyOn(api, "screenShareSoundApps").mockResolvedValue([
      { id: "sp", name: "Spotify", icon: null },
    ]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Share sound", hidden: true }));
    expect(await within(dialog).findByRole("alert", { hidden: true })).toHaveTextContent(
      "That app has stopped playing sound",
    );
    await waitFor(() => {
      expect(names(list)).toEqual(["Spotify", "No sound"]);
    });
    expect(dialog).toHaveAttribute("open");
    expect(await axeViolations(dialog)).toEqual([]);
  });

  it("is rendered outside any form", () => {
    const view = render(
      <form>
        <SoundAppPicker serverId={1} apps={APPS} />
      </form>,
    );
    expect(view.container.querySelector("dialog")).toBeNull();
    expect(screen.getAllByRole("dialog", { hidden: true }).at(-1)?.closest("form")).toBeNull();
  });
});
