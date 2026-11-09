import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { DiagnosticsCard } from "./DiagnosticsSection";

let api: MockApi;
let settings: Settings;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
  // jsdom decodes nothing, so every decoder starts as missing; tests opt in to what they need.
  vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("");
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

function onSystem(os: string) {
  vi.spyOn(api, "appInfo").mockResolvedValue({
    version: "0.8.0",
    packageVersion: "0.8.0",
    os,
    appimage: false,
    portable: false,
    limits: [],
    distro: null,
  });
}

async function renderCard() {
  const view = render(<DiagnosticsCard settings={settings} update={vi.fn()} />);
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

describe("DiagnosticsCard: media packages (FR-13.37)", () => {
  it("names the missing FreeBSD packages with their install line", async () => {
    onSystem("freebsd amd64");
    await renderCard();
    expect(await screen.findByText(/Missing media packages/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "sudo pkg install gstreamer1-libav gstreamer1-plugins-vpx gstreamer1-plugins-opus",
      ),
    ).toBeInTheDocument();
  });

  it("lists only the decoder that is really missing", async () => {
    onSystem("freebsd amd64");
    vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockImplementation((type: string) =>
      type.includes("vp9") ? "" : "probably",
    );
    await renderCard();
    expect(await screen.findByText("sudo pkg install gstreamer1-plugins-vpx")).toBeInTheDocument();
  });

  it("stays silent when every decoder answers and off FreeBSD", async () => {
    onSystem("freebsd amd64");
    vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("probably");
    const { unmount } = await renderCard();
    expect(screen.queryByText(/Missing media packages/)).toBeNull();
    unmount();

    vi.spyOn(HTMLMediaElement.prototype, "canPlayType").mockReturnValue("");
    onSystem("linux x86_64");
    await renderCard();
    expect(screen.queryByText(/Missing media packages/)).toBeNull();
  });
});
