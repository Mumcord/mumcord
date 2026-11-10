import i18next from "i18next";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { getToasts, resetNotifications } from "./notificationStore";
import { bindSuppressionNotices } from "./suppressionNotice";

let api: MockApi;
let unbind: () => void;

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  resetNotifications();
  unbind = bindSuppressionNotices(api, () => ({
    t: i18next.getFixedT("en", "notifications"),
    tSettings: i18next.getFixedT("en", "settings"),
  }));
});

afterEach(() => {
  unbind();
  resetAll();
});

describe("noise suppression fallback notice (#753)", () => {
  it("says that the standard level stands in", () => {
    act(() => {
      api.mock.emit("noiseSuppressionFallback", null);
    });
    expect(getToasts()).toEqual([
      expect.objectContaining({
        title: "Noise suppression too slow",
        text: "Too slow on this computer right now. Using “Standard” until it keeps up.",
      }),
    ]);
  });

  it("keeps one toast when it happens again", () => {
    act(() => {
      api.mock.emit("noiseSuppressionFallback", null);
      api.mock.emit("noiseSuppressionFallback", null);
    });
    expect(getToasts()).toHaveLength(1);
  });
});
