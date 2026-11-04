import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type SharePreview } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { axeViolations } from "../../../test/axe";
import { stubDialog } from "../../channels/testUtils";
import { ShareStartDialog } from "./ShareStartDialog";

let api: MockApi;

beforeAll(async () => {
  stubDialog();
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

const PREVIEW: SharePreview = {
  quality: "1080p30",
  width: 1920,
  height: 1080,
  fps: 30,
  kbps: 4000,
  encoder: { codec: "av1", gpu: null },
  softwareWarning: false,
  av1Skipped: null,
  path: { kind: "broadcast", maxViewers: 25 },
  cpuCores: null,
};

function previewWith(patch: Partial<SharePreview>) {
  return vi.spyOn(api, "screenSharePreview").mockResolvedValue({ ...PREVIEW, ...patch });
}

async function open({ soundAvailable = true } = {}) {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  const view = render(
    <ShareStartDialog
      serverId={1}
      soundAvailable={soundAvailable}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />,
  );
  const dialog = await screen.findByRole(
    "dialog",
    { name: "Share your screen" },
    { timeout: 5000 },
  );
  return { dialog, onCancel, onConfirm, view };
}

const viewersGet = () => screen.getByRole("status");

describe("ShareStartDialog", () => {
  it("opens with the defaults of Settings → Screen sharing", async () => {
    await api.settingsUpdate({
      ...(await api.settingsGet()),
      screenResolution: "1440",
      screenFps: "60",
      screenOptimize: "text",
      screenCodec: "vp9",
      screenUploadLimit: "10",
      screenSound: false,
    });
    await open();
    expect(screen.getByRole("button", { name: "1440p" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "60 fps" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("radio", { name: /Sharp text/ })).toBeChecked();
    expect(screen.getByRole("combobox", { name: "Codec" })).toHaveValue("vp9");
    expect(screen.getByRole("combobox", { name: "Upload limit" })).toHaveValue("10");
    expect(screen.getByRole("switch", { name: "Share the app's sound" })).not.toBeChecked();
  });

  it("hides the 4K option and the sound switch where the platform has no app sound", async () => {
    await open({ soundAvailable: false });
    expect(screen.queryByRole("button", { name: "Source" })).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("shows what viewers get and sends it once through the relay", async () => {
    previewWith({});
    await open();
    await waitFor(() => {
      expect(viewersGet()).toHaveTextContent(
        "Viewers get: up to 1080p · 30 fps · AV1 · about 4 Mbit/s",
      );
    });
    expect(viewersGet()).toHaveTextContent(
      "Sent once through the server's relay, which passes it on to up to 25 viewers. Viewers on an older Mumcord get VP9.",
    );
  });

  it("says one copy per viewer without fan-out and keeps small rates to one decimal", async () => {
    previewWith({
      height: 720,
      fps: 15,
      kbps: 1200,
      encoder: { codec: "vp9", gpu: null },
      path: { kind: "copies", maxViewers: 8 },
    });
    await open();
    await waitFor(() => {
      expect(viewersGet()).toHaveTextContent("up to 720p · 15 fps · VP9 · about 1.2 Mbit/s");
    });
    expect(viewersGet()).toHaveTextContent("One copy per viewer, up to 8.");
    expect(viewersGet()).not.toHaveTextContent("older Mumcord");
  });

  it("asks again for every change and names the encoder of each codec option", async () => {
    const preview = vi
      .spyOn(api, "screenSharePreview")
      .mockImplementation((_server, _choice, codec) =>
        Promise.resolve({
          ...PREVIEW,
          encoder: codec === "vp9" ? { codec: "vp9", gpu: null } : { codec: "av1", gpu: "nvidia" },
        }),
      );
    await open();
    const codec = screen.getByRole("combobox", { name: "Codec" });
    await waitFor(() => {
      expect(
        within(codec).getByRole("option", { name: "Auto: AV1 (graphics card)" }),
      ).toBeVisible();
    });
    expect(within(codec).getByRole("option", { name: "VP9" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "720p" }));
    fireEvent.click(screen.getByRole("radio", { name: /Smooth motion/ }));
    await waitFor(() => {
      expect(preview).toHaveBeenCalledWith(
        1,
        { resolution: "720", fps: "30", optimize: "motion", uploadLimit: "auto" },
        "vp9",
      );
    });
    expect(screen.getByRole("button", { name: "720p" })).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByText("Uses your graphics card, so sharing costs almost no CPU."),
    ).toBeVisible();
  });

  it("says how many cores the software encoder takes", async () => {
    previewWith({ cpuCores: { used: 3, total: 8 } });
    await open();
    const codec = screen.getByRole("combobox", { name: "Codec" });
    await waitFor(() => {
      expect(codec).toHaveAccessibleDescription(
        expect.stringContaining("Uses the processor: about 3 of 8 cores."),
      );
    });
  });

  it("names no core count when the engine does not know it", async () => {
    previewWith({});
    await open();
    await waitFor(() => {
      expect(viewersGet()).toHaveTextContent("Viewers get");
    });
    expect(screen.queryByText(/Uses the processor/)).toBeNull();
  });

  it.each([
    [
      { av1Skipped: "aboveSoftwareLimit" as const },
      "AV1 in software is limited to 1080p30 on this PC",
    ],
    [{ av1Skipped: "tooHeavy" as const }, "AV1 on the processor can't keep up on this PC"],
    [{ av1Skipped: "onBattery" as const }, "while your computer runs on battery"],
    [{ softwareWarning: true }, "AV1 runs on the processor here, with a higher CPU load."],
  ])("explains the encoder choice (%o)", async (patch, text) => {
    previewWith(patch);
    await open();
    const codec = screen.getByRole("combobox", { name: "Codec" });
    await waitFor(() => {
      expect(codec).toHaveAccessibleDescription(expect.stringContaining(text));
    });
  });

  it("notes the software limit for a manual AV1 pick above 1080p30", async () => {
    previewWith({});
    await open();
    fireEvent.change(screen.getByRole("combobox", { name: "Codec" }), {
      target: { value: "av1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "60 fps" }));
    expect(
      await screen.findByText("AV1 in software is limited to 1080p30 on this PC"),
    ).toBeVisible();
  });

  it("starts with the picks and stores them only when asked to", async () => {
    const save = vi.spyOn(api, "settingsUpdate");
    const { onConfirm } = await open();
    fireEvent.click(screen.getByRole("button", { name: "1080p" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Upload limit" }), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Choose what to share" }));
    expect(onConfirm).toHaveBeenCalledWith({
      choice: { resolution: "1080", fps: "30", optimize: "balanced", uploadLimit: "5" },
      codec: "auto",
      sound: true,
    });
    expect(save).not.toHaveBeenCalled();
  });

  it("stores the picks as the new defaults with Use these settings next time", async () => {
    const { onConfirm } = await open();
    fireEvent.click(screen.getByRole("button", { name: "15 fps" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Use these settings next time" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose what to share" }));
    expect(onConfirm).toHaveBeenCalled();
    await waitFor(async () => {
      expect((await api.settingsGet()).screenFps).toBe("15");
    });
  });

  it("starts with Enter and focuses the default button", async () => {
    const { onConfirm } = await open();
    const confirm = screen.getByRole("button", { name: "Choose what to share" });
    expect(confirm).toHaveFocus();
    fireEvent.submit(confirm);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("cancels with Cancel and Escape without starting", async () => {
    const { dialog, onCancel, onConfirm } = await open();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    act(() => {
      dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
    });
    expect(onCancel).toHaveBeenCalledTimes(2);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("falls back to the spec defaults when the settings cannot be read", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    api.mock.failNext("settingsGet", "internal", "db locked");
    await open();
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "30 fps" })).toHaveAttribute("aria-pressed", "true");
  });

  it("has no accessibility violations", async () => {
    previewWith({});
    const { view } = await open();
    await waitFor(() => {
      expect(viewersGet()).toHaveTextContent("Viewers get");
    });
    expect(await axeViolations(view.container)).toEqual([]);
  });
});
