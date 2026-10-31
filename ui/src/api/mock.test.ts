import { describe, expect, it, vi } from "vitest";
import { EngineError, type EventName, type EventPayloads } from "./client";
import { createMockApi, type MockApi } from "./mock";

function record<K extends EventName>(api: MockApi, event: K): EventPayloads[K][] {
  const seen: EventPayloads[K][] = [];
  api.on(event, (payload) => seen.push(payload));
  return seen;
}

async function connected(): Promise<MockApi> {
  const api = createMockApi();
  await api.connect(1, null, null);
  return api;
}

async function expectCode(promise: Promise<unknown>, code: string): Promise<void> {
  const err: unknown = await promise.then(
    () => undefined,
    (e: unknown) => e,
  );
  expect(err).toBeInstanceOf(EngineError);
  expect((err as EngineError).code).toBe(code);
}

describe("mock engine", () => {
  it("starts fresh without identity, servers or finished wizard", async () => {
    const api = createMockApi({ seed: "fresh" });
    expect(api.kind).toBe("mock");
    expect(await api.identityGet()).toBeNull();
    expect(await api.serversList()).toEqual([]);
    expect((await api.settingsGet()).wizardCompleted).toBe(false);
    const identity = await api.identityCreate("Tobias", null);
    expect(identity.name).toBe("Tobias");
  });

  it("shows its fictional media data in the demo seeds", async () => {
    const api = createMockApi({ seed: "demo" });
    expect(await api.screenShareList(1)).toEqual([api.mediaDemo.share]);
    expect(await api.watchPartyGet(1)).toBe(api.mediaDemo.party);
    expect(await api.screenShareQuality(1)).toBe(api.mediaDemo.quality);
    expect(await api.screenShareWatchStats(1, 7)).toEqual({ path: "direct", rms: 0.42 });
    expect(await api.screenShareWatch(1, 7, undefined as never)).toBeNull();
    expect(api.mediaDemo.picture).toMatch(/^data:image\/webp;base64,/);
    const plain = createMockApi();
    expect(plain.mediaDemo.picture).toBe("");
    await expectCode(plain.screenShareWatch(1, 7, undefined as never), "notImplemented");
  });

  it("connects through connecting → connected and sends a snapshot", async () => {
    const api = createMockApi();
    const states = record(api, "connectionStateChanged");
    const synced = record(api, "serverSynced");
    await api.connect(1, ["Raid"], null);
    expect(states.map((s) => s.state.state)).toEqual(["connecting", "connected"]);
    const snapshot = synced[0]?.snapshot;
    expect(snapshot?.channels.map((c) => c.name)).toEqual(["Root", "Lobby", "Raid", "AFK"]);
    const self = snapshot?.users.find((u) => u.session === snapshot.selfSession);
    expect(self?.channelId).toBe(2);
    expect(await api.sessionSnapshot(1)).toEqual(snapshot);
  });

  it("reports connection stats only while connected", async () => {
    const api = createMockApi();
    expect(await api.connectionStats(1)).toBeNull();
    await api.connect(1, null, null);
    api.mock.setConnectionStats({ sendBitrateKbps: 40 });
    expect(await api.connectionStats(1)).toMatchObject({ transport: "udp", sendBitrateKbps: 40 });
    await api.disconnect(1);
    expect(await api.connectionStats(1)).toBeNull();
  });

  it("asks for certificate trust and fails with certificateRejected when declined", async () => {
    const api = createMockApi();
    api.mock.requireCertTrust(1, "OLD:FINGERPRINT");
    const prompts = record(api, "certTrustRequired");
    const states = record(api, "connectionStateChanged");
    await api.connect(1, null, null);
    expect(prompts[0]?.previousFingerprint).toBe("OLD:FINGERPRINT");
    await api.certTrustDecide(1, prompts[0]?.fingerprint ?? "", false);
    expect(states.at(-1)?.state).toEqual({
      state: "disconnected",
      error: { code: "certificateRejected", message: "certificate not trusted" },
    });
  });

  it("delivers connection-state events only to active subscribers", async () => {
    const api = createMockApi();
    const handler = vi.fn();
    const stop = api.on("connectionStateChanged", handler);
    stop();
    await api.connect(1, null, null);
    expect(handler).not.toHaveBeenCalled();
  });

  it("rejects commands that need a connection", async () => {
    const api = createMockApi();
    await expectCode(api.channelJoin(1, 1), "notConnected");
  });

  it("deafen implies mute and undeafen restores the previous mute state", async () => {
    const api = await connected();
    const users = record(api, "userUpserted");
    await api.setSelfDeaf(true);
    expect(users.at(-1)?.user).toMatchObject({ selfDeaf: true, selfMute: true });
    await api.setSelfDeaf(false);
    expect(users.at(-1)?.user).toMatchObject({ selfDeaf: false, selfMute: false });
  });

  it("validates per-person volume", async () => {
    const api = await connected();
    await api.personSetVolume(1, 2, 150);
    await expectCode(api.personSetVolume(1, 2, 201), "invalidInput");
  });

  it("reports permission denied, message too large and stores sent messages", async () => {
    const api = await connected();
    await expectCode(api.sendChannelMessage(1, 3, "hi"), "permissionDenied");
    await expectCode(api.sendChannelMessage(1, 1, "x".repeat(5001)), "messageTooLarge");
    const sent = await api.sendChannelMessage(1, 1, "<b>hi</b>");
    expect(sent.own).toBe(true);
    expect(await api.historyLoad(1, { kind: "channel", channelId: 1 }, null, 50)).toEqual([sent]);
  });

  it("follows channels up to the listener limit and counts unread messages", async () => {
    const api = createMockApi({ maxListeners: 1 });
    await api.connect(1, null, null);
    const unread = record(api, "unreadChanged");
    await api.channelFollow(1, 2);
    await expectCode(api.channelFollow(1, 3), "listenerLimitReached");
    api.mock.receiveMessage({ kind: "channel", channelId: 2 }, "raid starts", 4);
    expect(unread.at(-1)).toEqual({
      serverId: 1,
      conversation: { kind: "channel", channelId: 2 },
      count: 1,
    });
    await api.conversationMarkRead(1, { kind: "channel", channelId: 2 });
    expect(unread.at(-1)?.count).toBe(0);
  });

  it("reports serverTooOld for following on servers before 1.4", async () => {
    const api = createMockApi({ serverVersion: "1.3.4" });
    await api.connect(1, null, null);
    await expectCode(api.channelFollow(1, 2), "serverTooOld");
  });

  it("does not store history when saving is off and clears history per scope", async () => {
    const api = await connected();
    const lobby = { kind: "channel", channelId: 1 } as const;
    await api.sendChannelMessage(1, 1, "kept");
    await api.historyClear({ kind: "server", serverId: 1 });
    expect(await api.historyLoad(1, lobby, null, 50)).toEqual([]);
    await api.historySetSaving(1, false);
    await api.sendChannelMessage(1, 1, "not kept");
    expect(await api.historyLoad(1, lobby, null, 50)).toEqual([]);
  });

  it("manages saved servers without exposing passwords", async () => {
    const api = createMockApi();
    const added = await api.serverAdd({
      label: "Raid night",
      address: "voice.example.test",
      port: 64738,
      username: "Zoe",
      password: { kind: "set", value: "secret" },
    });
    expect(added).not.toHaveProperty("password");
    expect(added.hasPassword).toBe(true);
    await api.serversReorder([added.id, 1]);
    expect((await api.serversList()).map((s) => s.id)).toEqual([added.id, 1]);
    await expectCode(api.serversReorder([1]), "invalidInput");
  });

  it("parses mumble:// links", async () => {
    const api = createMockApi();
    expect(await api.parseMumbleLink("mumble://anna@example.test:1234/Gaming/Raid%201")).toEqual({
      address: "example.test",
      port: 1234,
      username: "anna",
      password: null,
      channelPath: ["Gaming", "Raid 1"],
    });
    await expectCode(api.parseMumbleLink("https://example.test"), "invalidInput");
  });

  it("injects failures for the next call only", async () => {
    const api = createMockApi();
    api.mock.failNext("serversList", "secureStorageUnavailable");
    await expectCode(api.serversList(), "secureStorageUnavailable");
    expect(await api.serversList()).toHaveLength(1);
  });

  it("simulates reconnecting", async () => {
    const api = await connected();
    const states = record(api, "connectionStateChanged");
    api.mock.loseConnection();
    expect(states.at(-1)?.state.state).toBe("reconnecting");
    await api.reconnectCancel(1);
    expect(states.at(-1)?.state).toEqual({ state: "disconnected", error: null });
  });
});
