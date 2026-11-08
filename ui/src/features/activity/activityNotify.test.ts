import { renderHook } from "@testing-library/react";
import i18next from "i18next";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { resetNotifications, useToasts } from "../notifications/notificationStore";
import { handleActivity, noticeBody, switchOf } from "./activityNotify";
import type { ActivityEvent } from "./activityStore";

const SERVER = 1;
const share: ActivityEvent = {
  serverId: SERVER,
  at: 1_000,
  kind: "screenShare",
  name: "Ben",
  self: false,
  started: true,
  channelName: null,
  live: { kind: "screenShare", session: 7 },
};
const join: ActivityEvent = {
  serverId: SERVER,
  at: 2_000,
  kind: "connected",
  name: "Ada",
  self: false,
};

let api: MockApi;
const t = () => i18next.getFixedT("en", "activity");
const context = () => ({ t: t(), openLabel: "Open", serverName: () => "Night Owls" });

beforeEach(async () => {
  await applyLanguage("en");
  resetNotifications();
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  setApi(undefined);
});

describe("activity notifications (#597)", () => {
  it("never notifies own, earlier or stopped events", () => {
    expect(switchOf(share)).toBe("screenShare");
    expect(switchOf({ ...share, self: true })).toBeNull();
    expect(switchOf({ ...share, started: false })).toBeNull();
    expect(switchOf({ ...share, earlier: true })).toBeNull();
    expect(switchOf(join)).toBe("joins");
  });

  it("writes who did what, and where", () => {
    expect(noticeBody(share, t())).toBe("Ben started a screen share");
    expect(noticeBody({ ...share, channelName: "Lobby" }, t())).toBe(
      "Ben started a screen share in #Lobby",
    );
  });

  it("shows a toast while the window has focus", async () => {
    api.mock.setWindowFocused(true);
    await handleActivity(share, context());
    const { result } = renderHook(() => useToasts());
    expect(result.current).toEqual([
      expect.objectContaining({ title: "Night Owls", text: "Ben started a screen share" }),
    ]);
  });

  it("shows a system notification in the background", async () => {
    const settings = await api.settingsGet();
    await api.settingsUpdate({
      ...settings,
      activityNotify: { ...settings.activityNotify, screenShare: "both" },
    });
    api.mock.setWindowFocused(false);
    await handleActivity(share, context());
    expect(api.mock.activityNotices()).toEqual([
      expect.objectContaining({ serverId: SERVER, body: "Ben started a screen share" }),
    ]);
  });

  it("follows the mode per kind (#660): off is quiet, in-app is a toast, system notifies", async () => {
    api.mock.setWindowFocused(false);
    const settings = await api.settingsGet();
    const set = (joins: "off" | "inApp" | "desktop") =>
      api.settingsUpdate({ ...settings, activityNotify: { ...settings.activityNotify, joins } });
    await set("off");
    await handleActivity(join, context());
    expect(api.mock.activityNotices()).toEqual([]);
    await set("desktop");
    await handleActivity(join, context());
    expect(api.mock.activityNotices()).toEqual([expect.objectContaining({ kind: "joins" })]);
  });

  it("stays quiet during Do Not Disturb", async () => {
    api.mock.setWindowFocused(false);
    await api.dndSet(true);
    await handleActivity(share, context());
    expect(api.mock.activityNotices()).toEqual([]);
  });
});
