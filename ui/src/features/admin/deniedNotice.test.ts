import i18next from "i18next";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi, ServerDenied } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { getToasts, resetNotifications } from "../notifications/notificationStore";
import { bindDeniedNotices, deniedText } from "./deniedNotice";

let api: MockApi;
let unbind: () => void;
const t = () => i18next.getFixedT("en", "admin");

function denied(patch: Partial<ServerDenied>): ServerDenied {
  return {
    serverId: 1,
    error: { code: "permissionDenied", message: "" },
    channelId: null,
    kind: "permission",
    permission: null,
    handled: false,
    ...patch,
  };
}

beforeEach(async () => {
  api = await connectedMock({ bindStore: true });
  resetNotifications();
  unbind = bindDeniedNotices(api, () => ({ t: t(), serverName: () => "Night Owls" }));
});

afterEach(() => {
  unbind();
  resetAll();
});

describe("server refusals in plain words (#248)", () => {
  it("names the missing right and the channel", () => {
    expect(deniedText(t(), denied({ permission: "move", channelId: 1 }))).toMatch(
      /^You may not move people in .+\.$/,
    );
    expect(deniedText(t(), denied({ permission: "kick" }))).toBe("You may not kick people.");
  });

  it("words every other kind and keeps the server's own text", () => {
    expect(deniedText(t(), denied({ kind: "channelFull" }))).toBe("The channel is full.");
    expect(deniedText(t(), denied({ kind: "missingCertificate" }))).toBe(
      "This needs a client certificate.",
    );
    expect(
      deniedText(
        t(),
        denied({ kind: "text", error: { code: "permissionDenied", message: "Nope" } }),
      ),
    ).toBe("Nope");
    expect(deniedText(t(), denied({ kind: "unknown" }))).toBe("The server refused this.");
  });

  it("says a refusal once when the server repeats it for every person", () => {
    // Moving three people into a full channel is refused three times, one per person (issue #255).
    act(() => {
      for (let person = 0; person < 3; person += 1) {
        api.mock.emit("serverDenied", denied({ kind: "channelFull", channelId: 1 }));
      }
    });
    expect(getToasts()).toEqual([expect.objectContaining({ text: "The channel is full." })]);
  });

  it("shows a notice unless a dialog already shows the refusal", () => {
    act(() => {
      api.mock.emit("serverDenied", denied({ permission: "kick", handled: true }));
    });
    expect(getToasts()).toEqual([]);
    act(() => {
      api.mock.emit("serverDenied", denied({ kind: "channelFull" }));
    });
    expect(getToasts()).toEqual([
      expect.objectContaining({
        title: "Night Owls did not allow this",
        text: "The channel is full.",
      }),
    ]);
  });
});
