import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type Settings } from "../../api";
import { DEMO_MUMBLE } from "../../api/mock";
import { applyLanguage } from "../../i18n";
import { SettingsImport } from "./SettingsImport";

let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
});

afterEach(() => {
  setApi(undefined);
});

async function setup(mumble: typeof DEMO_MUMBLE | null) {
  const api = createMockApi({ seed: "demo", mumble });
  setApi(api);
  settings = await api.settingsGet();
  const update = vi.fn();
  render(<SettingsImport settings={settings} update={update} />);
  return { api, update };
}

describe("Settings → Servers → Import from Mumble (issue #66)", () => {
  it("says so when Mumble is not installed", async () => {
    await setup(null);
    fireEvent.click(screen.getByRole("button", { name: "Import from Mumble…" }));
    expect(
      await screen.findByText("The official Mumble client was not found on this computer."),
    ).toHaveAttribute("role", "status");
  });

  it("previews, skips servers already in Mumcord and saves the chosen settings", async () => {
    const { api, update } = await setup(DEMO_MUMBLE);
    fireEvent.click(screen.getByRole("button", { name: "Import from Mumble…" }));
    await screen.findByRole("dialog", { name: "Import from Mumble" });
    expect(screen.getByText("6 found · 1 already in Mumcord (skipped)")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity (client certificate)" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Volume and voice settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    await screen.findByRole("dialog", { name: "Import finished" });
    expect(screen.getByText("5 servers added to favourites")).toBeInTheDocument();
    expect(screen.getByText("1 server skipped")).toBeInTheDocument();
    expect(screen.getByText("Book Club is already in your list")).toBeInTheDocument();
    expect(update).toHaveBeenCalledWith(
      expect.objectContaining({ pttKey: "Ctrl+Space", outputVolumePercent: 80 }),
    );
    expect((await api.serversList()).map((s) => s.label)).toContain("Nordic Gamers");
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
