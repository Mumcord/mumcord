import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chooseOption } from "../../../components/SelectTestUtils";
import { createMockApi, setApi, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import type { InGameGame, InGameOverlayApi, InGameState } from "./inGameOverlay";
import { InGameOverlayCard } from "./OverlaySection";

let api: MockApi;

const game = (over: Partial<InGameGame>): InGameGame => ({
  id: "g",
  name: "Game",
  icon: null,
  detected: false,
  override: null,
  blocked: null,
  options: { corner: "topLeft", size: 100, opacity: 90 },
  ...over,
});

function fakeInGame(
  initial: Omit<InGameState, "explanationKey" | "openglLaunchOption"> & Partial<InGameState>,
) {
  const state: InGameState = { explanationKey: null, openglLaunchOption: null, ...initial };
  const fake: InGameOverlayApi = {
    state: vi.fn(() => Promise.resolve(state)),
    setMode: vi.fn(() => Promise.resolve(state)),
    setGameOverride: vi.fn(() => Promise.resolve(state)),
    setGameOptions: vi.fn(() => Promise.resolve(state)),
    subscribe: vi.fn(() => () => undefined),
  };
  return fake;
}

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("In-game overlay", () => {
  it("shows the OpenGL launch option and copies it", async () => {
    const option =
      "LD_PRELOAD=/home/u/.local/share/app.mumcord.client/overlay-layer/current/lib.so %command%";
    const writeText = vi.spyOn(api, "copyText").mockResolvedValue(null);
    render(
      <InGameOverlayCard
        inGameApi={fakeInGame({
          supported: true,
          mode: "on",
          games: [],
          openglLaunchOption: option,
        })}
      />,
    );
    expect(await screen.findByRole("textbox", { name: "Steam launch option" })).toHaveValue(option);
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(option);
    });
    expect(await screen.findByText("Copied")).toBeInTheDocument();
  });

  it("hides the OpenGL launch option without one", async () => {
    render(
      <InGameOverlayCard inGameApi={fakeInGame({ supported: true, mode: "on", games: [] })} />,
    );
    await screen.findByRole("radio", { name: "On" });
    expect(screen.queryByText("OpenGL games")).toBeNull();
  });

  it("sets the global mode", async () => {
    const fake = fakeInGame({ supported: true, mode: "off", games: [] });
    render(<InGameOverlayCard inGameApi={fake} />);
    fireEvent.click(await screen.findByRole("radio", { name: "On" }));
    expect(fake.setMode).toHaveBeenCalledWith("on");
    expect(screen.getByRole("radio", { name: "Off" })).toBeChecked();
  });

  it("shows the fallback note where it is unsupported", async () => {
    render(
      <InGameOverlayCard inGameApi={fakeInGame({ supported: false, mode: "off", games: [] })} />,
    );
    expect(await screen.findByText(/not available on this system/)).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "On" })).not.toBeInTheDocument();
  });

  it("names the reason when the overlay library is missing", async () => {
    render(
      <InGameOverlayCard
        inGameApi={fakeInGame({
          supported: false,
          explanationKey: "overlay.inGame.limits.libraryMissing",
          mode: "off",
          games: [],
        })}
      />,
    );
    expect(await screen.findByText(/overlay library .* was not found/)).toBeInTheDocument();
  });

  it("shows a platform hint and no anti-cheat warning without a blocklist", async () => {
    render(
      <InGameOverlayCard
        inGameApi={fakeInGame({
          supported: true,
          explanationKey: "overlay.inGame.limits.windowsFullscreen",
          mode: "on",
          games: [game({ name: "Some Game" })],
        })}
      />,
    );
    expect(
      await screen.findByText("Use borderless or windowed fullscreen in the game."),
    ).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "On" })).toBeChecked();
    expect(screen.queryByText(/anti-cheat protection/)).not.toBeInTheDocument();
  });

  it("follows changes made elsewhere", async () => {
    const fake = fakeInGame({ supported: true, mode: "off", games: [] });
    let push: (state: InGameState) => void = () => undefined;
    vi.mocked(fake.subscribe).mockImplementation((onChange) => {
      push = onChange;
      return () => undefined;
    });
    render(<InGameOverlayCard inGameApi={fake} />);
    await screen.findByRole("radio", { name: "Off", checked: true });
    act(() => {
      push({
        supported: true,
        explanationKey: null,
        openglLaunchOption: null,
        mode: "on",
        games: [],
      });
    });
    expect(screen.getByRole("radio", { name: "On" })).toBeChecked();
  });

  it("uses the engine commands by default", async () => {
    render(<InGameOverlayCard />);
    fireEvent.click(await screen.findByRole("radio", { name: "On" }));
    await waitFor(() => {
      expect(screen.getByRole("radio", { name: "On" })).toBeChecked();
    });
    expect((await api.ingameOverlayState()).mode).toBe("on");
  });

  it("switches a normal game and edits its options", async () => {
    const fake = fakeInGame({
      supported: true,
      mode: "on",
      games: [game({ id: "f", name: "Factorio", detected: true })],
    });
    render(<InGameOverlayCard inGameApi={fake} />);
    const toggle = await screen.findByRole("switch", { name: "Factorio" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(fake.setGameOverride).toHaveBeenCalledWith("f", false);

    fireEvent.click(screen.getByRole("button", { name: "Options for Factorio" }));
    const group = screen.getByRole("group", { name: "Factorio" });
    fireEvent.change(within(group).getByLabelText("Size"), { target: { value: "150" } });
    expect(fake.setGameOptions).toHaveBeenCalledWith("f", {
      corner: "topLeft",
      size: 150,
      opacity: 90,
    });
  });

  it("keeps an option changed while the previous save is still pending", async () => {
    const fake = fakeInGame({
      supported: true,
      mode: "on",
      games: [game({ id: "f", name: "Factorio", detected: true })],
    });
    fake.setGameOptions = vi.fn(() => new Promise<InGameState>(() => undefined));
    render(<InGameOverlayCard inGameApi={fake} />);
    fireEvent.click(await screen.findByRole("button", { name: "Options for Factorio" }));
    const group = screen.getByRole("group", { name: "Factorio" });
    chooseOption(within(group).getByRole("combobox", { name: "Position" }), "Bottom right");
    fireEvent.change(within(group).getByLabelText("Opacity"), { target: { value: "80" } });
    expect(fake.setGameOptions).toHaveBeenLastCalledWith("f", {
      corner: "bottomRight",
      size: 100,
      opacity: 80,
    });
  });

  it("asks for confirmation before enabling a blocklisted game", async () => {
    const fake = fakeInGame({
      supported: true,
      mode: "on",
      games: [game({ id: "v", name: "Valorant", blocked: { reason: "Vanguard" } })],
    });
    render(<InGameOverlayCard inGameApi={fake} />);
    const toggle = await screen.findByRole("switch", { name: "Valorant" });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("Blocked by anti-cheat.")).toBeInTheDocument();
    expect(screen.getByText(/Vanguard/)).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(fake.setGameOverride).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(fake.setGameOverride).not.toHaveBeenCalled();

    fireEvent.click(toggle);
    fireEvent.click(await screen.findByRole("button", { name: "Enable anyway" }));
    expect(fake.setGameOverride).toHaveBeenCalledWith("v", true);
  });
});
