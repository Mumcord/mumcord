import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type LinuxDistro, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { CodecHint } from "./CodecHint";
import type { PlaybackFailure } from "./codecSupport";

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
  // jsdom decodes nothing; make that explicit so the tests do not depend on it.
  vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("");
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

function onSystem(os: string, distro: LinuxDistro | null) {
  vi.spyOn(api, "appInfo").mockResolvedValue({
    version: "0.4.0",
    packageVersion: "0.4.0",
    os,
    appimage: true,
    portable: false,
    distro,
  });
}

async function renderHint(failure: PlaybackFailure | null) {
  const view = render(<CodecHint failure={failure} />);
  // Let the app info arrive.
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

const H264_TEXT = "This video needs H.264 support. Install gst-libav:";

describe("CodecHint", () => {
  it("shows the distribution's command after a YouTube HTML5 error on Linux", async () => {
    onSystem("linux x86_64", { id: "ubuntu", idLike: ["debian"] });
    await renderHint({ kind: "youtube", code: 5 });
    const hint = await screen.findByRole("status");
    expect(hint).toHaveTextContent(`${H264_TEXT} sudo apt install gstreamer1.0-libav`);
    expect(hint).not.toHaveTextContent("RPM Fusion");
  });

  it("adds the RPM Fusion note on Fedora and is generic on unknown systems", async () => {
    onSystem("linux x86_64", { id: "fedora", idLike: [] });
    const { unmount } = await renderHint({ kind: "twitch" });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "sudo dnf install gstreamer1-libav" + "On Fedora, gst-libav comes from the RPM Fusion",
    );
    unmount();

    onSystem("linux aarch64", null);
    await renderHint({ kind: "twitch" });
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Install gst-libav (the GStreamer libav plugin) with your system's package manager.",
    );
  });

  it("stays hidden for other errors, without a failure, with H.264 and off Linux", async () => {
    onSystem("linux x86_64", { id: "arch", idLike: [] });
    const other = await renderHint({ kind: "youtube", code: 150 });
    const none = await renderHint(null);
    expect(screen.queryByRole("status")).toBeNull();
    other.unmount();
    none.unmount();

    vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("probably");
    const decodes = await renderHint({ kind: "twitch" });
    expect(screen.queryByRole("status")).toBeNull();
    decodes.unmount();

    vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("");
    onSystem("macos aarch64", null);
    await renderHint({ kind: "twitch" });
    expect(screen.queryByRole("status")).toBeNull();
  });
});
