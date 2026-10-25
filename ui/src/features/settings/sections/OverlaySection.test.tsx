import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

function fakeInGame(initial: InGameState) {
  const state = { ...initial };
  const fake: InGameOverlayApi = {
    state: vi.fn(() => Promise.resolve(state)),
    setMode: vi.fn(() => Promise.resolve(state)),
    setGameOverride: vi.fn(() => Promise.resolve(state)),
    setGameOptions: vi.fn(() => Promise.resolve(state)),
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
