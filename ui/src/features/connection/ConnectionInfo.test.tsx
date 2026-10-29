import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import type { AudioHealth } from "../../bindings/commands";
import "../../i18n";
import { axeViolations } from "../../test/axe";
import { getServerState, resetServerStore } from "../channels/serverStore";
import { ConnectionInfo } from "./ConnectionInfo";
import { POLL_INTERVAL_MS } from "./useConnectionStats";

let api: MockApi;

async function setup(connect = true) {
  api = createMockApi({ seed: "ready" });
  setApi(api);
  // The stats are asked for the server the server store knows.
  getServerState();
  if (connect) {
    const [server] = await api.serversList();
    await api.connect((server ?? { id: 0 }).id, null, null);
  }
  return render(<ConnectionInfo />);
}

function openPanel() {
  fireEvent.click(screen.getByRole("button", { name: "Connection info" }));
  return screen.getByRole("dialog", { name: "Connection info" });
}

function valueOf(panel: HTMLElement, label: string) {
  return within(panel).getByText(label).closest("div")?.querySelector("dd")?.textContent;
}

/** Audio health with nothing wrong, overridden per test. */
function audio(overrides: Partial<AudioHealth> = {}): AudioHealth {
  return {
    dropouts: 0,
    lateFrames: 0,
    concealedFrames: 0,
    jitterBufferMs: 30,
    processingAvgUs: 300,
    processingMaxUs: 900,
    realtime: true,
    rating: "good",
    issue: null,
    windowSecs: 120,
    recentDropouts: 0,
    recentLateFrames: 0,
    recentConcealedFrames: 0,
    recentReceivedFrames: 0,
    recentProcessingMaxUs: 0,
    dropoutsHistory: Array<number>(12).fill(0),
    lateFramesHistory: Array<number>(12).fill(0),
    concealedHistory: Array<number>(12).fill(0),
    processingMaxHistoryUs: Array<number>(12).fill(0),
    ...overrides,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  resetServerStore();
  setApi(undefined);
});

describe("ConnectionInfo", () => {
  it("shows transport, latency, loss and bitrate", async () => {
    await setup();
    api.mock.setConnectionStats({ rttMs: 31.6, lossPercent: 2, sendBitrateKbps: 64 });
    const button = screen.getByRole("button", { name: "Connection info" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    const panel = openPanel();
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(button).toHaveAttribute("aria-controls", panel.id);
    await within(panel).findByText("Transport");
    expect(valueOf(panel, "Transport")).toBe("UDP");
    expect(valueOf(panel, "Latency")).toBe("32 ms");
    expect(valueOf(panel, "Packet loss")).toBe("2 %");
    expect(valueOf(panel, "Send bitrate")).toBe("64 kbit/s");
    expect(valueOf(panel, "Server limit")).toBe("558 kbit/s");
  });

  it("shows the audio counters with their recent figures in the details", async () => {
    await setup();
    api.mock.setConnectionStats({
      audio: audio({
        dropouts: 2,
        lateFrames: 7,
        concealedFrames: 12,
        jitterBufferMs: 60,
        processingAvgUs: 350,
        processingMaxUs: 4200,
        realtime: false,
        recentDropouts: 3,
        recentConcealedFrames: 21,
        recentReceivedFrames: 979,
        recentProcessingMaxUs: 3800,
      }),
    });
    const panel = openPanel();
    fireEvent.click(await within(panel).findByRole("button", { name: "Details" }));
    expect(valueOf(panel, "Audio dropouts")).toBe("2");
    expect(valueOf(panel, "Late audio frames")).toBe("7");
    expect(valueOf(panel, "Concealed audio frames")).toBe("12");
    expect(valueOf(panel, "Jitter buffer")).toBe("60 ms");
    expect(valueOf(panel, "Audio processing (avg / max)")).toBe("0.4 / 4.2 ms");
    expect(valueOf(panel, "Real-time audio")).toBe("Off");
    expect(within(panel).getByText(/Counts since connected/u)).toBeInTheDocument();
    const info = (label: string) => within(panel).getByRole("button", { name: `About ${label}` });
    expect(info("Audio dropouts")).toHaveAccessibleDescription(
      /Normal: 0\. Last 2 min: 1\.5 per minute\./u,
    );
    expect(info("Concealed audio frames")).toHaveAccessibleDescription(
      /Last 2 min: 2\.1 % of received frames\./u,
    );
    expect(info("Audio processing (avg / max)")).toHaveAccessibleDescription(
      /Last 2 min: max 3\.8 ms of the 10 ms frame budget\./u,
    );
    expect(info("Audio dropouts")).toHaveAttribute("data-tooltip");
  });

  it.each([
    ["good", null, "Audio: Good", "No problems in the last 2 minutes"],
    ["fair", "network", "Audio: Fair", "Some voice from others was concealed: network loss"],
    ["poor", "cpu", "Audio: Poor", "Your audio processing is running late: high CPU load"],
    ["poor", "device", "Audio: Poor", "Your audio device had dropouts"],
  ] as const)(
    "rates the audio %s (%s) with an icon and the reason",
    async (rating, issue, title, reason) => {
      await setup();
      api.mock.setConnectionStats({ audio: audio({ rating, issue }) });
      const panel = openPanel();
      const heading = await within(panel).findByText(title);
      const box = heading.closest(".audio-rating");
      expect(box).toHaveAttribute("data-rating", rating);
      expect(box?.querySelector("svg[aria-hidden='true']")).not.toBeNull();
      expect(within(panel).getByText(reason)).toBeInTheDocument();
    },
  );

  it("rounds the good window to whole minutes", async () => {
    await setup();
    api.mock.setConnectionStats({ audio: audio({ windowSecs: 50 }) });
    const panel = openPanel();
    expect(await within(panel).findByText("No problems in the last minute")).toBeInTheDocument();
  });

  it("keeps the details collapsed by default and remembers when they are opened", async () => {
    localStorage.removeItem("mumcord.audioDetailsOpen");
    await setup();
    let panel = openPanel();
    const toggle = await within(panel).findByRole("button", { name: "Details" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(panel).queryByText("Audio dropouts")).not.toBeVisible();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(panel).getByText("Audio dropouts")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Connection info" }));
    panel = openPanel();
    expect(await within(panel).findByRole("button", { name: "Details" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    localStorage.removeItem("mumcord.audioDetailsOpen");
  });

  it("still works when the storage is unavailable", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    await setup();
    const panel = openPanel();
    const toggle = await within(panel).findByRole("button", { name: "Details" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
  });

  it("leaves the audio rows out on a server without voice", async () => {
    await setup();
    api.mock.setConnectionStats({ audio: null });
    const panel = openPanel();
    await within(panel).findByText("Transport");
    expect(within(panel).queryByText("Audio dropouts")).toBeNull();
    expect(within(panel).queryByText(/^Audio: /u)).toBeNull();
  });

  it("updates live while open, e.g. when the bitrate drops", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await setup();
    const panel = openPanel();
    await within(panel).findByText("Send bitrate");
    expect(valueOf(panel, "Send bitrate")).toBe("72 kbit/s");
    api.mock.setConnectionStats({ sendBitrateKbps: 40, transport: "tcp", rttMs: null });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS);
    });
    expect(valueOf(panel, "Send bitrate")).toBe("40 kbit/s");
    expect(valueOf(panel, "Transport")).toBe("TCP (tunnelled)");
    expect(valueOf(panel, "Latency")).toBe("–");
  });

  it("stops polling when closed", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await setup();
    const spy = vi.spyOn(api, "connectionStats");
    openPanel();
    await waitFor(() => {
      expect(spy).toHaveBeenCalledTimes(1);
    });
    fireEvent.click(screen.getByRole("button", { name: "Connection info" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 3);
    });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("is keyboard operable: focus moves into the panel and Escape returns it", async () => {
    await setup();
    const button = screen.getByRole("button", { name: "Connection info" });
    button.focus();
    const panel = openPanel();
    expect(panel).toHaveFocus();
    fireEvent.keyDown(panel, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });

  it("closes on a click outside", async () => {
    await setup();
    openPanel();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("says so when not connected", async () => {
    await setup(false);
    const panel = openPanel();
    expect(await within(panel).findByText("Not connected.")).toBeInTheDocument();
  });

  it("reports a failed request", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    await setup();
    api.mock.failNext("connectionStats", "internal");
    const panel = openPanel();
    expect(
      await within(panel).findByText("Connection info is not available right now."),
    ).toBeInTheDocument();
  });

  it("has no axe violations when open, with the details shown", async () => {
    const { container } = await setup();
    const panel = openPanel();
    fireEvent.click(await within(panel).findByRole("button", { name: "Details" }));
    expect(await axeViolations(container)).toEqual([]);
  });
});
