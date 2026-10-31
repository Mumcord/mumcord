import { beforeEach, describe, expect, it, vi } from "vitest";
import { EngineError } from "./client";
import { createTauriApi } from "./tauri";

const invoke = vi.hoisted(() => vi.fn());
const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/event")>()),
  listen,
}));

describe("tauri client", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it("passes arguments through and resolves with the data", async () => {
    invoke.mockResolvedValue([]);
    const api = createTauriApi();
    expect(await api.historyLoad(1, { kind: "channel", channelId: 3 }, null, 20)).toEqual([]);
    expect(invoke).toHaveBeenCalledWith("history_load", {
      serverId: 1,
      conversation: { kind: "channel", channelId: 3 },
      before: null,
      limit: 20,
    });
  });

  it("sends a command only after the listeners subscribed before it are registered", async () => {
    // An event between a state fetch and the registration of its listener would be lost: the
    // fetch misses the change and the listener never sees it.
    let registered!: (unlisten: () => void) => void;
    listen.mockReturnValue(
      new Promise((resolve) => {
        registered = resolve;
      }),
    );
    invoke.mockResolvedValue([]);
    const api = createTauriApi();
    api.on("connectionStateChanged", () => undefined);
    const listed = api.connectionsList();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(listen).toHaveBeenCalledWith("connection-state-changed", expect.any(Function));
    expect(invoke).not.toHaveBeenCalled();
    registered(() => undefined);
    expect(await listed).toEqual([]);
    expect(invoke).toHaveBeenCalledWith("connections_list");
  });

  it("still sends commands when registering a listener fails", async () => {
    listen.mockRejectedValue(new Error("no IPC"));
    invoke.mockResolvedValue([]);
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const api = createTauriApi();
    api.on("connectionStateChanged", () => undefined);
    expect(await api.connectionsList()).toEqual([]);
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });

  it("rejects with an EngineError carrying the engine's error code", async () => {
    invoke.mockRejectedValue({ code: "notImplemented", message: "not implemented yet" });
    const err: unknown = await createTauriApi()
      .serversList()
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(EngineError);
    expect(err).toMatchObject({ code: "notImplemented" });
  });
});
