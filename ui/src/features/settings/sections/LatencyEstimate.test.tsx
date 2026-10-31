import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LatencyEstimate } from "../../../bindings/commands";
import "../../../i18n";
import { formatMs } from "./formatMs";
import { LatencyBreakdown, LatencySummary } from "./LatencyEstimate";

const ESTIMATE: LatencyEstimate = {
  lowLatency: true,
  packetMs: 10,
  inputBufferUs: 5300,
  encodeUs: 15500,
  networkUs: 12250,
  jitterUs: 10000,
  processingUs: 1100,
  outputBufferUs: 5300,
  totalUs: 38000,
  otherModeTotalUs: 92000,
};

describe("LatencyEstimate", () => {
  it("uses the decimal separator of the language", () => {
    expect(formatMs(12250, "de")).toBe("12,3");
  });

  it("formats microseconds as milliseconds with at most one decimal", () => {
    expect(formatMs(38000, "en")).toBe("38");
    expect(formatMs(12250, "en")).toBe("12.3");
    expect(formatMs(1049, "en")).toBe("1");
  });

  it("compares low-latency mode with normal mode", () => {
    render(<LatencySummary estimate={ESTIMATE} />);
    expect(
      screen.getByText("≈ 38 ms mouth to ear (estimate) · normal mode ≈ 92 ms"),
    ).toBeInTheDocument();
  });

  it("compares normal mode with low-latency mode", () => {
    render(<LatencySummary estimate={{ ...ESTIMATE, lowLatency: false }} />);
    expect(screen.getByText(/· low-latency mode ≈ 92 ms$/u)).toBeInTheDocument();
  });

  it("draws every stage as a focusable bar segment as wide as its share", () => {
    render(<LatencyBreakdown estimate={ESTIMATE} />);
    const bar = screen.getByRole("group", {
      name: "Latency breakdown: Microphone buffer 5.3 ms, Opus frame 10 ms + encoder look-ahead 15.5 ms, Network (half of the ping) 12.3 ms, Jitter buffer 10 ms, Processing 1.1 ms, Speaker buffer 5.3 ms",
    });
    const segments = within(bar).getAllByRole("img");
    expect(segments).toHaveLength(6);
    const mic = within(bar).getByRole("img", { name: "Microphone buffer 5.3 ms" });
    expect(mic).toHaveAttribute("tabindex", "0");
    expect(mic).toHaveAttribute("data-tooltip", "Microphone buffer 5.3 ms");
    expect(mic.style.flexGrow).toBe(String(ESTIMATE.inputBufferUs));
    expect(screen.getByText("≈ 38 ms")).toBeInTheDocument();
  });

  it("leaves the network out of the bar when there is no ping yet", () => {
    render(<LatencyBreakdown estimate={{ ...ESTIMATE, networkUs: null }} />);
    const bar = screen.getByRole("group", { name: /Network \(half of the ping\) not connected/ });
    expect(within(bar).getAllByRole("img")).toHaveLength(5);
  });
});
