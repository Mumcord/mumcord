import { describe, expect, it } from "vitest";
import { stripMessage, type StripInput } from "./stripMessage";

const base: StripInput = {
  error: null,
  connection: "connected",
  connected: true,
  voiceServerId: 1,
  shownServerId: 1,
};

describe("stripMessage (#246)", () => {
  it("shows the own voice channel when nothing else applies", () => {
    expect(stripMessage(base)).toBe("voice");
  });

  it("follows the documented priority when several states apply at once", () => {
    const all: StripInput = {
      error: "Mute change failed",
      connection: "reconnecting",
      connected: false,
      voiceServerId: 2,
      shownServerId: 1,
    };
    expect(stripMessage(all)).toBe("error");
    expect(stripMessage({ ...all, error: null })).toBe("reconnecting");
    expect(stripMessage({ ...all, error: null, connection: "connecting" })).toBe("connecting");
    expect(stripMessage({ ...all, error: null, connection: "disconnected" })).toBe("disconnected");
    expect(stripMessage({ ...all, error: null, connection: "connected", connected: true })).toBe(
      "voiceElsewhere",
    );
  });

  it("says read-only while connected without voice anywhere", () => {
    expect(stripMessage({ ...base, voiceServerId: null })).toBe("readOnly");
    // Not connected wins over the missing voice server.
    expect(
      stripMessage({ ...base, voiceServerId: null, connected: false, connection: "disconnected" }),
    ).toBe("disconnected");
  });

  it("shows voice on another server, also while the shown server is not connected", () => {
    expect(stripMessage({ ...base, voiceServerId: 2 })).toBe("voiceElsewhere");
    expect(
      stripMessage({ ...base, voiceServerId: 2, shownServerId: 3, connection: "disconnected" }),
    ).toBe("voiceElsewhere");
  });
});
