import { beforeEach, describe, expect, it, vi } from "vitest";
import { EngineError } from "./client";
import { createTauriApi } from "./tauri";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("tauri client", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("passes arguments through and resolves with the data", async () => {
    invoke.mockResolvedValue([]);
    const api = createTauriApi();
    expect(await api.historyLoad({ kind: "channel", channelId: 3 }, null, 20)).toEqual([]);
    expect(invoke).toHaveBeenCalledWith("history_load", {
      conversation: { kind: "channel", channelId: 3 },
      before: null,
      limit: 20,
    });
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
