import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { LowLatencyGroup } from "./LowLatencyGroup";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("LowLatencyGroup", () => {
  it("is off by default, shows no warning and saves the switch", async () => {
    const update = vi.fn();
    render(<LowLatencyGroup settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", { name: "Low-latency mode Beta feature" });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("Beta feature")).toBeInTheDocument();
    expect(screen.queryByRole("note")).toBeNull();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ lowLatency: true });
    await screen.findByText(/low-latency mode ≈ 47\.9 ms/u);
  });

  it("warns about the trade-offs while on", async () => {
    render(<LowLatencyGroup settings={{ ...settings, lowLatency: true }} update={vi.fn()} />);
    const note = screen.getByRole("note");
    expect(note).toHaveTextContent("Trades robustness for speed");
    expect(note).toHaveTextContent("10 ms packets");
    expect(note).toHaveTextContent("use a cable");
    expect(note).toHaveTextContent("echo cancellation is off");
    expect(note).toHaveTextContent("Push-to-talk");
    await screen.findByText(/low-latency mode ≈ 47\.9 ms/u);
  });

  it("refreshes the estimate every two seconds and logs failures", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const spy = vi.spyOn(api, "latencyEstimate");
    const log = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const view = render(<LowLatencyGroup settings={settings} update={vi.fn()} />);
    await screen.findByText(/mouth to ear/u);
    spy.mockRejectedValueOnce(new Error("no device"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(log).toHaveBeenCalledWith("Loading the latency estimate failed", expect.any(Error));
    view.unmount();
    const calls = spy.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6000);
    });
    expect(spy.mock.calls.length).toBe(calls);
  });
});
