/**
 * The sound line in the sharer's live bar (FR-7.85, AC-7.63, AC-7.71): what the share's sound
 * carries, or "No sound" with its reason as tooltip and accessible description.
 */
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ScreenShare } from "../../api";
import { axeViolations } from "../../test/axe";
import { connectedMock, resetAll } from "../channels/testUtils";
import { SharerBar } from "./SharerBar";
import { resetScreenShareStore } from "./sharesStore";

const SELF = 1;

function share(patch: Partial<ScreenShare>): ScreenShare {
  return {
    session: SELF,
    paused: false,
    sound: false,
    soundAvailable: false,
    soundMode: "none",
    soundApp: null,
    soundReason: null,
    soundChoice: null,
    viewers: [],
    blocked: null,
    relayReason: null,
    kind: null,
    title: null,
    elapsedMs: 0,
    ...patch,
  };
}

async function showing(patch: Partial<ScreenShare>, os = "linux x86_64"): Promise<Element> {
  const api = await connectedMock({ bindStore: true });
  const info = api.appInfo.bind(api);
  vi.spyOn(api, "appInfo").mockImplementation(async () => ({ ...(await info()), os }));
  api.mock.emit("voiceServerChanged", { serverId: 1 });
  const { container } = render(<SharerBar />);
  act(() => {
    api.mock.emit("screenSharesChanged", { serverId: 1, shares: [share(patch)] });
  });
  await screen.findByRole("group", { name: "Your screen share" });
  return container;
}

beforeEach(() => {
  resetScreenShareStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  resetScreenShareStore();
  resetAll();
});

const cases: {
  name: string;
  patch: Partial<ScreenShare>;
  os?: string;
  label: string;
  reason: RegExp;
}[] = [
  {
    name: "desktop",
    patch: { sound: true, soundAvailable: true, soundMode: "desktop" },
    label: "Sound: desktop",
    reason: /Whole screen: desktop sound without Mumcord/,
  },
  {
    name: "app",
    patch: { sound: true, soundAvailable: true, soundMode: "app", soundApp: "Firefox" },
    label: "Sound: Firefox",
    reason: /One window: only that app's sound/,
  },
  {
    name: "switch off",
    patch: { sound: false, soundAvailable: true, soundMode: "desktop", soundReason: "off" },
    label: "No sound",
    reason: /The Sound switch is off/,
  },
  {
    // A Linux window share started with the switch off is never asked "Which app's sound?".
    name: "switch off without a source",
    patch: { soundMode: "none", soundReason: "off" },
    label: "No sound",
    reason: /The Sound switch is off/,
  },
  {
    name: "no sound chosen",
    patch: { soundMode: "none", soundReason: "notChosen" },
    label: "No sound",
    reason: /No sound.{1,3}was chosen for this window/,
  },
  {
    name: "old Windows",
    patch: { soundReason: "osTooOld" },
    os: "windows x86_64",
    label: "No sound",
    reason: /needs Windows 10 version 2004 or newer/,
  },
  {
    name: "old macOS",
    patch: { soundReason: "osTooOld" },
    os: "macos aarch64",
    label: "No sound",
    reason: /needs macOS 13 or newer/,
  },
  {
    name: "no PipeWire",
    patch: { soundReason: "noPipeWire" },
    label: "No sound",
    reason: /needs PipeWire/,
  },
  {
    name: "capture failed",
    patch: { soundReason: "captureFailed" },
    label: "No sound",
    reason: /Audio capture failed\. The video share continues/,
  },
];

describe("sound line of the live bar", () => {
  it.each(cases)("$name: shows the line, its reason and passes axe", async (c) => {
    const container = await showing(c.patch, c.os);
    const badge = await screen.findByText(c.label, { selector: ".share-badge" });
    expect(badge).toHaveAccessibleDescription(c.reason);
    expect(badge.getAttribute("data-tooltip")).toMatch(c.reason);
    expect(badge).toHaveAttribute("tabindex", "0");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("draws the speaker outlined, as in mockup 10", async () => {
    await showing({ sound: true, soundAvailable: true, soundMode: "desktop" });
    const badge = await screen.findByText("Sound: desktop", { selector: ".share-badge" });
    expect(badge.querySelector("svg")).not.toBeNull();
    expect(badge.querySelector('[fill="currentColor"]')).toBeNull();
  });

  it("marks a failed capture as a warning", async () => {
    await showing({ soundReason: "captureFailed" });
    expect(await screen.findByText("No sound", { selector: ".share-badge" })).toHaveAttribute(
      "data-tone",
      "warn",
    );
  });
});
