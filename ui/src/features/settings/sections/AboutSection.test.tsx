import { act, render, screen } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { VersionCard } from "./AboutSection";

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

function fakeSettings(): Settings {
  return { whatsNewAuto: false } as Settings;
}

/** Renders the About card with an app info that reports the given platform limits. */
async function renderCard(limits: string[]) {
  vi.spyOn(api, "appInfo").mockResolvedValue({
    version: "0.8.0",
    packageVersion: "0.8.0",
    os: "FreeBSD 15.1",
    appimage: false,
    portable: false,
    distro: null,
    limits,
  } as never);
  const view = render(<VersionCard settings={fakeSettings()} update={() => {}} />);
  // Let the app info arrive.
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

describe("About section", () => {
  it("names what the platform cannot do", async () => {
    await renderCard(["camera", "screenShareSound"]);
    expect(await screen.findByText("Not available on FreeBSD")).toBeTruthy();
    expect(screen.getByText(/Camera: no camera backend/)).toBeTruthy();
    expect(screen.getByText(/Screen-share sound/)).toBeTruthy();
    expect(screen.getByText(/drm-66-kmod/)).toBeTruthy();
  });

  it("names the protected-video limit", async () => {
    await renderCard(["protectedVideo"]);
    expect(await screen.findByText(/Protected \(DRM\) video cannot play/)).toBeTruthy();
  });

  it("says nothing about limits where everything works", async () => {
    await renderCard([]);
    expect(screen.queryByText("Not available on FreeBSD")).toBeNull();
  });
});
