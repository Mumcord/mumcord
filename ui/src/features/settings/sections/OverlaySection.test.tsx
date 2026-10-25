import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import type { InGameGame, InGameOverlayApi, InGameState } from "./inGameOverlay";
import { OverlaySection } from "./OverlaySection";

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

function fakeInGame(initial: Omit<InGameState, "explanationKey"> & Partial<InGameState>) {
  const state: InGameState = { explanationKey: null, ...initial };
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

describe("Window overlay", () => {
  it("toggles, changes the corner and the sliders through the commands", async () => {
    const setEnabled = vi.spyOn(api, "overlaySetEnabled");
    const update = vi.spyOn(api, "overlayUpdate");
    const move = vi.spyOn(api, "overlayMoveMode");
    render(<OverlaySection inGameApi={fakeInGame({ supported: true, mode: "off", games: [] })} />);
    const toggle = await screen.findByRole("switch", { name: "Show the overlay" });
    fireEvent.click(toggle);
    expect(setEnabled).toHaveBeenCalledWith(true);
    await waitFor(() => {
      expect(toggle).toBeChecked();
    });

    fireEvent.change(screen.getByLabelText("Position", { selector: "select" }), {
      target: { value: "bottomRight" },
    });
    expect(update).toHaveBeenLastCalledWith(
      expect.objectContaining({ corner: "bottomRight", customPosition: null }),
    );
    fireEvent.change(screen.getByLabelText("Opacity"), { target: { value: "50" } });
    expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ opacity: 50 }));
    fireEvent.click(screen.getByRole("switch", { name: "Show the whole channel" }));
    expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ showWholeChannel: true }));

    fireEvent.click(screen.getByRole("button", { name: "Move overlay…" }));
    expect(move).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("button", { name: "Done moving" })).toBeInTheDocument();
  });

  it("explains the limits of this desktop", async () => {
    const state = await api.overlayState();
    vi.spyOn(api, "overlayState").mockResolvedValue({
      ...state,
      onTopSupported: false,
      positionSupported: false,
      explanationKey: "limits.wayland",
    });
    render(<OverlaySection />);
    expect(await screen.findByText(/Wayland desktop/)).toBeInTheDocument();
    expect(screen.getByText(/corner positions and moving have no effect/)).toBeInTheDocument();
    expect(screen.getByLabelText("Position", { selector: "select" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move overlay…" })).toBeDisabled();
  });
});

describe("KDE window rule", () => {
  it("is not offered outside KDE Wayland", async () => {
    render(<OverlaySection />);
    await screen.findByRole("switch", { name: "Show the overlay" });
    expect(screen.queryByRole("button", { name: "Set up automatically" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open KDE window rules" })).not.toBeInTheDocument();
  });

  it("sets up, removes and opens the rules on KDE Wayland", async () => {
    const state = await api.overlayState();
    vi.spyOn(api, "overlayState").mockResolvedValue({ ...state, kdeWayland: true });
    // The mock's own state is not on KDE Wayland, so answer with KDE states here.
    const install = vi
      .spyOn(api, "overlayKwinRuleInstall")
      .mockResolvedValue({ ...state, kdeWayland: true, kwinRuleInstalled: true });
    const remove = vi
      .spyOn(api, "overlayKwinRuleRemove")
      .mockResolvedValue({ ...state, kdeWayland: true, kwinRuleInstalled: false });
    const open = vi.spyOn(api, "overlayOpenKdeWindowRules");
    render(<OverlaySection />);
    expect(await screen.findByRole("button", { name: "Set up automatically" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Remove rule" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Set up automatically" }));
    expect(install).toHaveBeenCalledOnce();
    expect(await screen.findByText(/window rule .* is active/)).toBeVisible();

    fireEvent.click(await screen.findByRole("button", { name: "Remove rule" }));
    expect(remove).toHaveBeenCalledOnce();
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Remove rule" })).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Open KDE window rules" }));
    expect(open).toHaveBeenCalledOnce();
  });

  it("reports a failed setup", async () => {
    const state = await api.overlayState();
    vi.spyOn(api, "overlayState").mockResolvedValue({ ...state, kdeWayland: true });
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(api, "overlayKwinRuleInstall").mockRejectedValue(new Error("no"));
    render(<OverlaySection />);
    fireEvent.click(await screen.findByRole("button", { name: "Set up automatically" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be changed/);
  });
});

describe("In-game overlay", () => {
  it("sets the global mode", async () => {
    const fake = fakeInGame({ supported: true, mode: "off", games: [] });
    render(<OverlaySection inGameApi={fake} />);
    fireEvent.click(await screen.findByRole("radio", { name: "On" }));
    expect(fake.setMode).toHaveBeenCalledWith("on");
    expect(screen.getByRole("radio", { name: "Off" })).toBeChecked();
  });

  it("shows the fallback note where it is unsupported", async () => {
    render(<OverlaySection inGameApi={fakeInGame({ supported: false, mode: "off", games: [] })} />);
    expect(await screen.findByText(/not available on this system/)).toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "On" })).not.toBeInTheDocument();
  });

  it("names the reason when the overlay library is missing", async () => {
    render(
      <OverlaySection
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

  it("follows changes made elsewhere", async () => {
    const fake = fakeInGame({ supported: true, mode: "off", games: [] });
    let push: (state: InGameState) => void = () => undefined;
    vi.mocked(fake.subscribe).mockImplementation((onChange) => {
      push = onChange;
      return () => undefined;
    });
    render(<OverlaySection inGameApi={fake} />);
    await screen.findByRole("radio", { name: "Off", checked: true });
    act(() => {
      push({ supported: true, explanationKey: null, mode: "on", games: [] });
    });
    expect(screen.getByRole("radio", { name: "On" })).toBeChecked();
  });

  it("uses the engine commands by default", async () => {
    render(<OverlaySection />);
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
    render(<OverlaySection inGameApi={fake} />);
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
    render(<OverlaySection inGameApi={fake} />);
    fireEvent.click(await screen.findByRole("button", { name: "Options for Factorio" }));
    const group = screen.getByRole("group", { name: "Factorio" });
    fireEvent.change(within(group).getByLabelText("Position"), {
      target: { value: "bottomRight" },
    });
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
    render(<OverlaySection inGameApi={fake} />);
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
