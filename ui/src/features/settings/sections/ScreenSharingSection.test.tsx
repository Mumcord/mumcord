import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { CodecCard, HardwareCard, QualityDefaultsCard } from "./ScreenSharingSection";

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

function Page({ update }: { update: (patch: Partial<Settings>) => void }) {
  return (
    <>
      <QualityDefaultsCard settings={settings} update={update} />
      <CodecCard settings={settings} update={update} />
      <HardwareCard settings={settings} update={update} />
    </>
  );
}

describe("Screen sharing settings (FR-7.60)", () => {
  it("saves the quality defaults and hides the 4K option", () => {
    const update = vi.fn();
    render(<Page update={update} />);
    const resolution = screen.getByRole("combobox", { name: "Resolution" });
    expect(Array.from((resolution as HTMLSelectElement).options).map((o) => o.text)).toEqual([
      "Auto",
      "720p",
      "1080p",
      "1440p",
    ]);
    fireEvent.change(resolution, { target: { value: "1080" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Frame rate" }), {
      target: { value: "60" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Optimize for" }), {
      target: { value: "text" },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Upload limit" }), {
      target: { value: "5" },
    });
    expect(update.mock.calls).toEqual([
      [{ screenResolution: "1080" }],
      [{ screenFps: "60" }],
      [{ screenOptimize: "text" }],
      [{ screenUploadLimit: "5" }],
    ]);
  });

  it("saves the codec and the graphics card switch", () => {
    const update = vi.fn();
    render(<Page update={update} />);
    fireEvent.click(screen.getByRole("radio", { name: "VP9" }));
    fireEvent.click(screen.getByRole("switch", { name: /Use graphics card when available/ }));
    expect(update).toHaveBeenCalledWith({ screenCodec: "vp9" });
    expect(update).toHaveBeenCalledWith({ screenUseGpu: !settings.screenUseGpu });
  });

  it("shows the active encoder and checks again", async () => {
    vi.useFakeTimers();
    render(<Page update={vi.fn()} />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("status").textContent).toContain("Active encoder: AV1 (processor)");
    fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole("status").textContent).toBe("Checking…");
    expect(screen.getByRole("button", { name: "Check again" })).toBeDisabled();
    await act(async () => {
      await vi.runAllTimersAsync();
    });
    expect(screen.getByRole("status").textContent).toContain("Active encoder");
    vi.useRealTimers();
  });

  it("saves the ask-each-time switch", () => {
    const update = vi.fn();
    render(<Page update={update} />);
    const toggle = screen.getByRole("switch", { name: /Ask for these settings each time/ });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({ screenAskEachTime: false });
  });

  it("shows software playback when WebCodecs cannot decode", async () => {
    render(<Page update={vi.fn()} />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("AV1 playback").nextSibling?.textContent).toBe("Software");
    expect(screen.getByText("VP9 playback").nextSibling?.textContent).toBe("Software");
  });

  it("shows hardware playback when WebCodecs supports the codec", async () => {
    vi.stubGlobal("VideoDecoder", {
      isConfigSupported: (config: { codec: string }) =>
        Promise.resolve({ supported: config.codec.startsWith("vp09") }),
    });
    render(<Page update={vi.fn()} />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("AV1 playback").nextSibling?.textContent).toBe("Software");
    expect(screen.getByText("VP9 playback").nextSibling?.textContent).toBe("Hardware");
    vi.unstubAllGlobals();
  });

  it("has no axe violations", async () => {
    const { container } = render(<Page update={vi.fn()} />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(await axeViolations(container)).toEqual([]);
  });
});
