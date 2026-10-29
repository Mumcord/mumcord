import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
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
  return within(panel).getByText(label).nextElementSibling?.textContent;
}

afterEach(() => {
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

  it("shows the health of the local audio path while voice runs", async () => {
    await setup();
    api.mock.setConnectionStats({
      audio: {
        dropouts: 2,
        lateFrames: 7,
        concealedFrames: 12,
        jitterBufferMs: 60,
        processingAvgUs: 350,
        processingMaxUs: 4200,
        realtime: false,
      },
    });
    const panel = openPanel();
    await within(panel).findByText("Audio dropouts");
    expect(valueOf(panel, "Audio dropouts")).toBe("2");
    expect(valueOf(panel, "Late audio frames")).toBe("7");
    expect(valueOf(panel, "Concealed audio frames")).toBe("12");
    expect(valueOf(panel, "Jitter buffer")).toBe("60 ms");
    expect(valueOf(panel, "Audio processing (avg / max)")).toBe("0.4 / 4.2 ms");
    expect(valueOf(panel, "Real-time audio")).toBe("Off");
  });

  it("leaves the audio rows out on a server without voice", async () => {
    await setup();
    api.mock.setConnectionStats({ audio: null });
    const panel = openPanel();
    await within(panel).findByText("Transport");
    expect(within(panel).queryByText("Audio dropouts")).toBeNull();
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

  it("has no axe violations when open", async () => {
    const { container } = await setup();
    const panel = openPanel();
    await within(panel).findByText("Transport");
    expect(await axeViolations(container)).toEqual([]);
  });
});
