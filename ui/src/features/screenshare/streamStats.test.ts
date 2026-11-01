/** The frame meter of a watched share (issue #163, FR-3.126). */
import { describe, expect, it } from "vitest";
import { StreamMeter } from "./streamStats";

const BASE = 1000;

describe("stream meter", () => {
  it("counts the frames and the bytes of the last second", () => {
    const meter = new StreamMeter();
    for (let i = 0; i < 5; i += 1) {
      meter.frame({
        atMs: BASE + i * 200,
        timestampUs: i * 200 * 1000,
        bytes: 1000,
        width: 1920,
        height: 1080,
      });
    }
    const stats = meter.snapshot();
    expect(stats.fps).toBe(5);
    expect(stats.kbps).toBe(40);
    expect(stats.width).toBe(1920);
    expect(stats.height).toBe(1080);
  });

  it("keeps only the last second", () => {
    const meter = new StreamMeter();
    meter.frame({ atMs: BASE, timestampUs: 0, bytes: 1000, width: 1280, height: 720 });
    meter.frame({ atMs: BASE + 2000, timestampUs: 2000000, bytes: 1000, width: 1280, height: 720 });
    expect(meter.snapshot().fps).toBe(1);
  });

  it("counts dropped frames and the delay against the fastest one", () => {
    const meter = new StreamMeter();
    meter.frame({ atMs: BASE, timestampUs: 0, bytes: 500, width: 1280, height: 720 });
    meter.frame({ atMs: BASE + 600, timestampUs: 200000, bytes: 500, width: 1280, height: 720 });
    meter.drop(BASE + 700);
    const stats = meter.snapshot();
    expect(stats.dropped).toBe(1);
    expect(stats.delayMs).toBe(400);
  });

  it("says nothing about the delay without a frame", () => {
    expect(new StreamMeter().snapshot().delayMs).toBeNull();
  });

  it("counts decoded pictures for the size and the frame rate, but not for the bitrate", () => {
    const meter = new StreamMeter();
    meter.frame({ atMs: BASE, timestampUs: null, bytes: null, width: 1280, height: 720 });
    meter.frame({ atMs: BASE + 100, timestampUs: null, bytes: null, width: 1280, height: 720 });
    const stats = meter.snapshot();
    expect(stats.fps).toBe(2);
    expect(stats.height).toBe(720);
    expect(stats.kbps).toBeNull();
    expect(stats.delayMs).toBeNull();
  });

  it("gives the dropped frames as a share of all frames of the watch", () => {
    const meter = new StreamMeter();
    expect(meter.snapshot().droppedShare).toBeNull();
    for (let i = 0; i < 9; i += 1) {
      meter.frame({ atMs: BASE + i, timestampUs: i * 1000, bytes: 10, width: 640, height: 360 });
    }
    meter.drop(BASE + 10);
    expect(meter.snapshot().droppedShare).toBeCloseTo(0.1);
  });
});
