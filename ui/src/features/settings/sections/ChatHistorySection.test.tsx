import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { ExternalContentCard, HistorySyncCard } from "./ChatHistorySection";

let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  const api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  setApi(undefined);
});

describe("ExternalContentCard: full-size images (FR-1.74)", () => {
  it("are on by default and each switch turns off on its own", () => {
    const update = vi.fn();
    render(<ExternalContentCard settings={settings} update={update} />);
    const offer = screen.getByRole("switch", { name: "Offer my full-size images Beta feature" });
    const load = screen.getByRole("switch", {
      name: "Load full-size images from others Beta feature",
    });
    expect(offer).toBeChecked();
    expect(load).toBeChecked();
    fireEvent.click(offer);
    expect(update).toHaveBeenLastCalledWith({ offerFullImages: false });
    fireEvent.click(load);
    expect(update).toHaveBeenLastCalledWith({ loadFullImages: false });
  });

  it("marks both switches as beta", () => {
    render(<ExternalContentCard settings={settings} update={vi.fn()} />);
    for (const name of [
      "Offer my full-size images Beta feature",
      "Load full-size images from others Beta feature",
    ]) {
      const row = screen.getByRole("switch", { name }).closest(".settings-switch");
      expect(row).not.toBeNull();
      expect(within(row as HTMLElement).getByText("Beta feature")).toBeInTheDocument();
    }
  });
});

describe("HistorySyncCard: history sharing (FR-1.60)", () => {
  it("is on by default, explains itself behind a tip and turns sharing off", () => {
    const update = vi.fn();
    render(<HistorySyncCard settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Share my channel history" });
    expect(toggle).toBeChecked();
    expect(
      screen.getByRole("button", { name: "More information: Share my channel history" }),
    ).toHaveAttribute("data-tooltip", expect.stringMatching(/Private chats are never shared/));
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ historySharing: false });
  });

  it("turns sharing back on", () => {
    const update = vi.fn();
    render(<HistorySyncCard settings={{ ...settings, historySharing: false }} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Share my channel history" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ historySharing: true });
  });
});
