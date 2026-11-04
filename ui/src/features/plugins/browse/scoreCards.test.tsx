import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type PluginScored } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { setSettingsOpener } from "../../settings/settingsOpener";
import { PluginSafetyGroup, PluginSoundsGroup } from "../PlatformSettingsGroups";
import { resetPlatformSettings } from "../platformSettings";
import { resetPluginStore } from "../pluginStore";
import { forgetListingScores } from "../score/useListingScore";
import { PluginCard } from "./PluginCard";
import { indexed } from "./fixtures";

let api: MockApi;

const repo = { kind: "repo", repositoryId: 1, name: "Lantern Plugin Garden" } as const;
const scored = (value: number, band: "low" | "medium" | "high"): PluginScored => ({
  kind: "scored",
  score: { value, band, e: 0, f: 1, h: 0, factors: [], version: 1 },
});

beforeEach(async () => {
  await applyLanguage("en");
  resetPluginStore();
  resetPlatformSettings();
  forgetListingScores();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setSettingsOpener(null);
  setApi(undefined);
});

function card(plugin = indexed("notes", { source: repo })) {
  return render(
    <ul>
      <PluginCard plugin={plugin} installed={false} onDetails={vi.fn()} onInstall={vi.fn()} />
    </ul>,
  );
}

describe("plugin card score", () => {
  it("shows the chip of an unofficial plugin under the summary", async () => {
    vi.spyOn(api, "pluginScore").mockResolvedValue(scored(37, "medium"));
    const view = card();
    expect(
      await screen.findByText(/Security score 37 out of 100. Medium risk/),
    ).toBeInTheDocument();
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("shows no chip and asks for no score for an official plugin", () => {
    const spy = vi.spyOn(api, "pluginScore");
    card(indexed("dice"));
    expect(spy).not.toHaveBeenCalled();
    expect(screen.queryByText(/risk/)).toBeNull();
  });

  it("disables Install of a high-risk plugin while the safety setting is on", async () => {
    vi.spyOn(api, "pluginScore").mockResolvedValue(scored(72, "high"));
    await api.pluginSetPlatformSettings({
      blockHighRisk: true,
      relayDataEnabled: true,
      soundVolume: 70,
      soundMuted: false,
    });
    card();
    expect(await screen.findByText("Blocked by your safety setting")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install Notes" })).toBeDisabled();
  });

  it("keeps Install enabled for a high-risk plugin when the setting is off", async () => {
    vi.spyOn(api, "pluginScore").mockResolvedValue(scored(72, "high"));
    card();
    expect(await screen.findByText(/Security score 72 out of 100. High risk/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install Notes" })).toBeEnabled();
  });

  it("says what Mumcord version is needed and disables Install", () => {
    vi.spyOn(api, "pluginScore").mockResolvedValue(scored(10, "low"));
    const opener = vi.fn();
    setSettingsOpener(opener);
    card(indexed("notes", { source: repo, needsMumcord: "0.8.0" }));
    expect(screen.getByText("Needs Mumcord 0.8.0 or newer")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install Notes" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Update Mumcord" }));
    expect(opener).toHaveBeenCalledWith("general");
  });
});

describe("platform settings groups", () => {
  it("saves the safety switch and keeps the other settings", async () => {
    const save = vi.spyOn(api, "pluginSetPlatformSettings");
    const view = render(<PluginSafetyGroup />);
    const toggle = await screen.findByRole("switch", { name: /Don't allow high-risk plugins/ });
    await waitFor(() => {
      expect(toggle).toBeEnabled();
    });
    fireEvent.click(toggle);
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ blockHighRisk: true }));
    expect(save.mock.calls[0]?.[0]).toMatchObject({ soundVolume: 70, relayDataEnabled: true });
    expect(await axeViolations(view.container)).toEqual([]);
  });

  it("saves plugin sound volume and mute", async () => {
    const save = vi.spyOn(api, "pluginSetPlatformSettings");
    const view = render(<PluginSoundsGroup />);
    const mute = await screen.findByRole("switch", { name: /Mute plugin sounds/ });
    await waitFor(() => {
      expect(mute).toBeEnabled();
    });
    fireEvent.click(mute);
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ soundMuted: true }));
    const slider = screen.getByRole("slider", { name: "Plugin sound volume" });
    fireEvent.change(slider, { target: { value: "75" } });
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ soundVolume: 75 }));
    expect(await axeViolations(view.container)).toEqual([]);
  });
});
