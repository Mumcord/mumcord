import { describe, expect, it } from "vitest";
import type { ChannelInfo, ServerInfo } from "../../api";
import { getServerState, resetServerStore, type ServerState } from "../channels/serverStore";
import { pinnedInfo } from "./pinnedInfoModel";

const allow = {
  enter: true,
  textMessage: true,
  listen: true,
  registerSelf: false,
  speak: true,
  whisper: true,
  write: false,
};

function channel(id: number, patch: Partial<ChannelInfo> = {}): ChannelInfo {
  return {
    id,
    parentId: id === 0 ? null : 0,
    name: `C${String(id)}`,
    position: 0,
    permissions: allow,
    description: null,
    hasDescription: false,
    maxUsers: null,
    temporary: false,
    links: [],
    enterRestricted: false,
    ...patch,
  };
}

function state(channels: ChannelInfo[], info: Partial<ServerInfo> = {}): ServerState {
  resetServerStore();
  return {
    ...getServerState(),
    info: {
      version: "1.5",
      followSupported: true,
      welcomeText: "Hello <b>all</b>",
      defaultChannelId: 1,
      maxMessageLength: 0,
      maxImageMessageLength: 0,
      allowHtml: true,
      release: null,
      os: null,
      maxBandwidth: null,
      maxUsers: null,
      recordingAllowed: false,
      serverLabel: null,
      serverAddress: null,
      serverPort: null,
      tlsFingerprint: null,
      usersOnline: 0,
      sessionId: null,
      registered: false,
      currentChannelPermissions: null,
      mumcordUsers: 0,
      tlsTrusted: true,
      tlsIssuer: null,
      tlsValidFrom: null,
      tlsValidUntil: null,
      tlsCertificatePem: null,
      ...info,
    },
    channels: new Map(channels.map((c) => [c.id, c])),
  };
}

const at = (channelId: number) => ({ kind: "channel", channelId }) as const;

describe("pinnedInfo", () => {
  it("pins the welcome text in the root channel, below its description", () => {
    const s = state([channel(0, { description: "Root rules", hasDescription: true })]);
    expect(pinnedInfo(s, at(0)).blocks).toEqual(["Root rules", "Hello <b>all</b>"]);
  });

  it("pins the welcome text in the default channel only without a description", () => {
    expect(pinnedInfo(state([channel(0), channel(1)]), at(1)).blocks).toEqual(["Hello <b>all</b>"]);
    const described = state([channel(1, { description: "Lobby", hasDescription: true })]);
    expect(pinnedInfo(described, at(1)).blocks).toEqual(["Lobby"]);
    // Still being fetched: the description will replace the welcome text, so don't flash it.
    const pending = state([channel(1, { hasDescription: true })]);
    expect(pinnedInfo(pending, at(1))).toEqual({
      blocks: [],
      welcome: null,
      description: null,
      needsFetch: true,
    });
  });

  it("shows nothing for other channels without a description, blank texts and private chats", () => {
    const s = state([channel(2), channel(3, { description: "  ", hasDescription: false })], {
      welcomeText: " ",
    });
    expect(pinnedInfo(s, at(2))).toEqual({
      blocks: [],
      welcome: null,
      description: null,
      needsFetch: false,
    });
    expect(pinnedInfo(s, at(3)).blocks).toEqual([]);
    expect(pinnedInfo(s, at(9)).blocks).toEqual([]);
    expect(pinnedInfo(s, { kind: "privateSession", session: 1 }).blocks).toEqual([]);
  });
});
