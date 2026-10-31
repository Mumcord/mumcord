import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import type { View } from "../../app/view";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { ViewContext } from "../../app/view";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { getServerState, resetServerStore } from "../channels/serverStore";
import { ConnectionInfo } from "./ConnectionInfo";

let api: MockApi;

async function setup(view: Partial<View> = {}) {
  api = createMockApi({ seed: "ready" });
  vi.spyOn(api, "bitrateMax").mockResolvedValue(128);
  setApi(api);
  getServerState();
  const [server] = await api.serversList();
  await api.connect((server ?? { id: 0 }).id, null, null);
  api.mock.setConnectionStats({
    sendBitrateKbps: 64,
    serverMaxBandwidthKbps: 128,
    rttMs: 20,
    lossPercent: 1,
  });
  return render(
    <PreferencesProvider>
      <ViewContext value={view as View}>
        <ConnectionInfo />
      </ViewContext>
    </PreferencesProvider>,
  );
}

async function openQuick() {
  fireEvent.contextMenu(screen.getByRole("button", { name: "Connection info" }));
  const panel = screen.getByRole("dialog", { name: "Voice quality" });
  await within(panel).findByRole("slider");
  return panel;
}

afterEach(() => {
  vi.restoreAllMocks();
  resetServerStore();
  setApi(undefined);
});

describe("ConnectionQuickPanel", () => {
  it("opens on right-click, the menu key, Shift+F10 and the arrow button", async () => {
    await setup();
    const info = screen.getByRole("button", { name: "Connection info" });
    fireEvent.keyDown(info, { key: "ContextMenu" });
    expect(screen.getByRole("dialog", { name: "Voice quality" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.keyDown(info, { key: "F10", shiftKey: true });
    expect(screen.getByRole("dialog", { name: "Voice quality" })).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Voice quality" }));
    expect(screen.getByRole("dialog", { name: "Voice quality" })).toBeInTheDocument();
  });

  it("closes on outside click and on Escape, returning focus to the icon", async () => {
    await setup();
    await openQuick();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("dialog")).toBeNull();
    await openQuick();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: "Connection info" })).toHaveFocus();
  });

  it("keeps a left click on the statistics", async () => {
    await setup();
    fireEvent.click(screen.getByRole("button", { name: "Connection info" }));
    expect(screen.getByRole("dialog", { name: "Connection info" })).toBeInTheDocument();
  });

  it("binds the slider to the stored bitrate and shows limit and effective value", async () => {
    await setup();
    const panel = await openQuick();
    const slider = within(panel).getByRole("slider");
    const limitTip = await within(panel).findByRole("button", {
      name: "More information: Bitrate",
    });
    await waitFor(() => {
      expect(limitTip.dataset.tooltip).toMatch(/Maximum: 128 kbit\/s/);
    });
    expect(slider).toHaveAttribute("max", "128");
    await within(panel).findByText("Sending now");
    expect(
      within(panel).getByRole("button", { name: "More information: Sending now" }).dataset.tooltip,
    ).toMatch(/after adapting to packet loss/);
    expect(within(panel).getByText("64 kbit/s")).toBeVisible();
    fireEvent.change(slider, { target: { value: "32" } });
    await waitFor(async () => {
      expect((await api.settingsGet()).bitrateKbps).toBe(32);
    });
  });

  it("toggles Music quality in the stored settings", async () => {
    await setup();
    const panel = await openQuick();
    const music = await within(panel).findByRole("switch", { name: "Music quality" });
    expect(music).not.toBeChecked();
    fireEvent.click(music);
    await waitFor(async () => {
      expect((await api.settingsGet()).musicQuality).toBe(true);
    });
  });

  it("shows the estimated bandwidth against the server limit", async () => {
    await setup();
    const panel = await openQuick();
    // 64 kbit/s voice plus 14.8 kbit/s of headers for 20 ms packets.
    await within(panel).findByText("≈ 79 of 128 kbit/s");
    expect(within(panel).getByRole("meter")).toHaveAccessibleName(/Estimated bandwidth/);
    expect(
      within(panel).getByRole("button", { name: /More information: Estimated bandwidth/ }).dataset
        .tooltip,
    ).toMatch(/With low-latency mode: ≈ 93 kbit\/s \(10 ms packets\), still within the limit/);
  });

  it("switches low-latency mode and counts its 10 ms packets in the bandwidth", async () => {
    await setup();
    const panel = await openQuick();
    await within(panel).findByText("≈ 79 of 128 kbit/s");
    const row = within(panel).getByRole("switch", { name: /Low-latency mode/ });
    expect(row).not.toBeChecked();
    fireEvent.click(row);
    await waitFor(async () => {
      expect((await api.settingsGet()).lowLatency).toBe(true);
    });
    await within(panel).findByText("≈ 93 of 128 kbit/s");
    expect(within(panel).queryByText(/With low-latency mode/)).toBeNull();
    const tip = (topic: string) =>
      within(panel).getByRole("button", { name: `More information: ${topic}` }).dataset.tooltip;
    expect(tip("Estimated bandwidth")).toMatch(/Turn the mode off to save ≈ 14 kbit\/s/);
    const warning = tip("Trades robustness for speed");
    expect(warning).toMatch(/Uses more bandwidth: ≈ 93 kbit\/s instead of ≈ 79\./);
    expect(warning).toMatch(/Use headphones: echo cancellation is off\./);
    expect(within(panel).queryByText(/Use headphones/)).toBeNull();
    const music = within(panel).getByRole("switch", { name: "Music quality" });
    expect(music).toBeDisabled();
    expect(music).toBeChecked();
    expect(tip("Music quality")).toMatch(/Included while low-latency mode is on/);
    await within(panel).findByText(/normal mode ≈ 107\.1 ms/);
    expect(within(panel).getByRole("group", { name: /latency/i })).toBeVisible();
  });

  it("previews the latency of the other mode while off", async () => {
    await setup();
    const panel = await openQuick();
    await within(panel).findByText(/mouth to ear/);
    expect(within(panel).getByText(/low-latency mode ≈/)).toBeVisible();
    expect(within(panel).queryByText("Trades robustness for speed")).toBeNull();
    expect(within(panel).queryByRole("group", { name: /latency/i })).toBeNull();
  });

  it("explains the options behind ? buttons instead of lines of text", async () => {
    await setup();
    const panel = await openQuick();
    await within(panel).findByRole("switch", { name: "Music quality" });
    const music = within(panel).getByRole("button", { name: "More information: Music quality" });
    expect(music.dataset.tooltip).toMatch(/Tunes the encoder for music/);
    expect(within(panel).queryByText(/Tunes the encoder for music/)).toBeNull();
    const low = within(panel).getByRole("button", { name: "More information: Low-latency mode" });
    expect(low.dataset.tooltip).toMatch(/For playing music together in real time/);
  });

  it("keeps the audio health rating visible", async () => {
    await setup();
    const panel = await openQuick();
    await within(panel).findByText("Ping 20 ms, loss 1 %");
    expect(within(panel).getByText("Audio: Fair")).toBeInTheDocument();
  });

  it("opens the audio settings and closes", async () => {
    const openAudioSettings = vi.fn();
    await setup({ openAudioSettings });
    const panel = await openQuick();
    fireEvent.click(within(panel).getByRole("button", { name: "Voice settings…" }));
    expect(openAudioSettings).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("has no accessibility violations", async () => {
    const { container } = await setup();
    await openQuick();
    expect(await axeViolations(container)).toEqual([]);
  });
});
