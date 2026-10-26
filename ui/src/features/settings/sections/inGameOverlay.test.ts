import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  type GameOverlayEntry,
  type InGameOverlayState,
  type MockApi,
} from "../../../api";
import { createInGameOverlayApi } from "./inGameOverlay";

let api: MockApi;

const blocklist = {
  name: "Valorant",
  antiCheat: "Riot Vanguard",
  reason: "Bans for overlays",
  source: "https://example.invalid",
};

const entry = (over: Partial<GameOverlayEntry>): GameOverlayEntry => ({
  key: { kind: "steam", value: 570 },
  name: "Dota 2",
  enabled: false,
  riskConfirmed: false,
  corner: "bottomRight",
  sizePercent: 120,
  opacityPercent: 70,
  showWholeChannel: true,
  showChannelName: false,
  blocklist: null,
  effective: "gameOff",
  ...over,
});

const raw: InGameOverlayState = {
  supported: true,
  explanationKey: null,
  openglLaunchOption: null,
  mode: "on",
  games: [entry({})],
  known: [
    { key: { kind: "exe", value: "valorant.exe" }, name: "Valorant", running: false, blocklist },
    { key: { kind: "exe", value: "factorio" }, name: "factorio", running: true, blocklist: null },
  ],
  running: [{ kind: "exe", value: "factorio" }],
};

beforeEach(() => {
  api = createMockApi();
  vi.spyOn(api, "ingameOverlayState").mockResolvedValue(raw);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("createInGameOverlayApi", () => {
  it("maps configured and known games from the engine state", async () => {
    const state = await createInGameOverlayApi(api).state();
    expect(state.supported).toBe(true);
    expect(state.explanationKey).toBeNull();
    expect(state.mode).toBe("on");
    expect(state.games).toEqual([
      {
        id: "steam:570",
        name: "Dota 2",
        icon: null,
        detected: false,
        override: false,
        blocked: null,
        options: { corner: "bottomRight", size: 120, opacity: 70 },
      },
      {
        id: "exe:valorant.exe",
        name: "Valorant",
        icon: null,
        detected: false,
        override: null,
        blocked: { reason: "Riot Vanguard: Bans for overlays" },
        options: { corner: "topLeft", size: 100, opacity: 90 },
      },
      {
        id: "exe:factorio",
        name: "factorio",
        icon: null,
        detected: true,
        override: null,
        blocked: null,
        options: { corner: "topLeft", size: 100, opacity: 90 },
      },
    ]);
  });

  it("marks a configured game as running too", async () => {
    vi.spyOn(api, "ingameOverlayState").mockResolvedValue({
      ...raw,
      running: [{ kind: "steam", value: 570 }],
    });
    const state = await createInGameOverlayApi(api).state();
    expect(state.games.find((g) => g.id === "steam:570")?.detected).toBe(true);
  });

  it("passes the platform limitation through", async () => {
    vi.spyOn(api, "ingameOverlayState").mockResolvedValue({
      ...raw,
      supported: false,
      explanationKey: "overlay.inGame.limits.libraryMissing",
    });
    const state = await createInGameOverlayApi(api).state();
    expect(state.supported).toBe(false);
    expect(state.explanationKey).toBe("overlay.inGame.limits.libraryMissing");
  });

  it("sets the global mode", async () => {
    const setMode = vi.spyOn(api, "ingameOverlaySetMode").mockResolvedValue(raw);
    await createInGameOverlayApi(api).setMode("off");
    expect(setMode).toHaveBeenCalledWith("off");
  });

  it("keeps a configured game's other options when toggling it", async () => {
    const setGame = vi.spyOn(api, "ingameOverlaySetGame").mockResolvedValue(raw);
    const inGame = createInGameOverlayApi(api);
    await inGame.state();
    await inGame.setGameOverride("steam:570", true);
    expect(setGame).toHaveBeenCalledWith(
      expect.objectContaining({
        key: { kind: "steam", value: 570 },
        enabled: true,
        riskConfirmed: false,
        corner: "bottomRight",
        sizePercent: 120,
        opacityPercent: 70,
        showWholeChannel: true,
        showChannelName: false,
      }),
    );
  });

  it("confirms the risk when enabling a blocklisted game", async () => {
    const setGame = vi.spyOn(api, "ingameOverlaySetGame").mockResolvedValue(raw);
    const inGame = createInGameOverlayApi(api);
    await inGame.state();
    await inGame.setGameOverride("exe:valorant.exe", true);
    expect(setGame).toHaveBeenCalledWith(
      expect.objectContaining({
        key: { kind: "exe", value: "valorant.exe" },
        name: "Valorant",
        enabled: true,
        riskConfirmed: true,
        corner: "topLeft",
        sizePercent: 100,
        opacityPercent: 90,
      }),
    );
  });

  it("goes back to the default without losing the look", async () => {
    const setGame = vi.spyOn(api, "ingameOverlaySetGame").mockResolvedValue(raw);
    const inGame = createInGameOverlayApi(api);
    await inGame.state();
    await inGame.setGameOverride("steam:570", null);
    expect(setGame).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: null, riskConfirmed: false, sizePercent: 120 }),
    );
  });

  it("saves per-game options", async () => {
    const setGame = vi.spyOn(api, "ingameOverlaySetGame").mockResolvedValue(raw);
    const inGame = createInGameOverlayApi(api);
    await inGame.state();
    await inGame.setGameOptions("exe:factorio", { corner: "topRight", size: 150, opacity: 50 });
    expect(setGame).toHaveBeenCalledWith(
      expect.objectContaining({
        key: { kind: "exe", value: "factorio" },
        enabled: null,
        corner: "topRight",
        sizePercent: 150,
        opacityPercent: 50,
      }),
    );
  });

  it("rejects a game it does not know", async () => {
    const inGame = createInGameOverlayApi(api);
    await inGame.state();
    await expect(inGame.setGameOverride("exe:nope", true)).rejects.toThrow();
  });

  it("forwards change events", async () => {
    const inGame = createInGameOverlayApi(api);
    const seen: string[] = [];
    const stop = inGame.subscribe((state) => {
      seen.push(state.mode);
    });
    await api.ingameOverlaySetMode("on");
    expect(seen).toEqual(["on"]);
    stop();
    await api.ingameOverlaySetMode("off");
    expect(seen).toEqual(["on"]);
  });
});
