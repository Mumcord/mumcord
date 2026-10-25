import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
import { PreferencesProvider } from "../../../app/PreferencesProvider";
import { createMockApi, setApi, type MockApi, type Settings } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { QuickSettings } from "../../userpanel/QuickSettings";
import { useSettings } from "../useSettings";
import { PositionalGroup } from "./PositionalGroup";

let api: MockApi;
let settings: Settings;
let saves: MockInstance<MockApi["settingsUpdate"]>;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  settings = await api.settingsGet();
  saves = vi.spyOn(api, "settingsUpdate");
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

/** The group as the settings screen mounts it: loaded and saved through the engine. */
function Screen() {
  const { settings: loaded, update } = useSettings();
  return loaded ? <PositionalGroup settings={loaded} update={update} /> : null;
}

/** The positional switch of the last settings record handed to the engine. */
function lastSaved(): boolean | undefined {
  return saves.mock.lastCall?.[0].positional.enabled;
}

const SWITCH = { name: "Positional audio" };

describe("PositionalGroup", () => {
  it("is off by default and turns positional audio on", () => {
    const update = vi.fn();
    render(<PositionalGroup settings={settings} update={update} />);
    const toggle = screen.getByRole("switch", SWITCH);
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(update).toHaveBeenCalledWith({
      positional: { enabled: true, minDistance: 1, maxDistance: 15 },
    });
  });

  it("saves valid distances and flags invalid ones without saving", () => {
    const update = vi.fn();
    render(<PositionalGroup settings={settings} update={update} />);
    const max = screen.getByLabelText("Silent beyond (metres)");
    fireEvent.change(max, { target: { value: "40" } });
    expect(update).toHaveBeenLastCalledWith({
      positional: { enabled: false, minDistance: 1, maxDistance: 40 },
    });
    expect(screen.queryByRole("alert")).toBeNull();

    update.mockClear();
    fireEvent.change(max, { target: { value: "1" } });
    expect(update).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("Enter distances in metres");
    expect(max).toHaveAttribute("aria-invalid", "true");

    fireEvent.change(max, { target: { value: "1001" } });
    expect(update).not.toHaveBeenCalled();
    fireEvent.change(max, { target: { value: "" } });
    expect(update).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Full volume up to (metres)"), {
      target: { value: "-1" },
    });
    expect(update).not.toHaveBeenCalled();
  });

  it("says no game is detected while no source delivers positions", async () => {
    render(<PositionalGroup settings={settings} update={vi.fn()} />);
    expect(await screen.findByText("No game detected")).toBeInTheDocument();
  });

  it("shows the source and game, and follows status changes", async () => {
    vi.spyOn(api, "positionalStatus").mockResolvedValue({
      enabled: true,
      source: "Mumble Link",
      game: "Factorio",
      senders: [],
    });
    render(<PositionalGroup settings={settings} update={vi.fn()} />);
    expect(await screen.findByText("Positions from: Mumble Link – Factorio")).toBeInTheDocument();

    act(() => {
      api.mock.emit("positionalStatusChanged", {
        status: { enabled: true, source: "Plugin X", game: null, senders: [] },
      });
    });
    expect(screen.getByText("Positions from: Plugin X")).toBeInTheDocument();

    act(() => {
      api.mock.emit("positionalStatusChanged", {
        status: { enabled: true, source: null, game: null, senders: [] },
      });
    });
    expect(screen.getByText("No game detected")).toBeInTheDocument();
  });

  it("marks the group as beta with a hint", () => {
    render(<PositionalGroup settings={settings} update={vi.fn()} />);
    const heading = screen.getByRole("heading", { name: /Positional audio/u });
    expect(within(heading).getByText("Beta")).toBeInTheDocument();
    expect(within(heading).getByText("Beta feature")).toBeInTheDocument();
    expect(screen.getByText(/depend on the game/u)).toBeInTheDocument();
  });

  it("marks the quick settings row as beta", async () => {
    render(
      <PreferencesProvider>
        <QuickSettings
          menu={{ x: 0, y: 0, opener: null }}
          account={{
            connected: false,
            registered: false,
            canRegister: false,
            onEditProfile: vi.fn(),
            onRegister: vi.fn(),
          }}
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
        />
      </PreferencesProvider>,
    );
    const row = await screen.findByRole("menuitemcheckbox", { name: /^Positional audio/u });
    expect(within(row).getByText("Beta")).toBeInTheDocument();
  });

  it("keeps the quick settings switch and the settings switch in step", async () => {
    const quick = render(
      <PreferencesProvider>
        <QuickSettings
          menu={{ x: 0, y: 0, opener: null }}
          account={{
            connected: false,
            registered: false,
            canRegister: false,
            onEditProfile: vi.fn(),
            onRegister: vi.fn(),
          }}
          onClose={vi.fn()}
          onOpenSettings={vi.fn()}
        />
      </PreferencesProvider>,
    );
    const quickSwitch = await screen.findByRole("menuitemcheckbox", { name: /^Positional audio/u });
    expect(quickSwitch).toHaveAttribute("aria-checked", "false");
    fireEvent.click(quickSwitch);
    await waitFor(() => {
      expect(lastSaved()).toBe(true);
    });
    quick.unmount();

    // The settings screen reads the same setting and changes it back.
    render(
      <PreferencesProvider>
        <Screen />
      </PreferencesProvider>,
    );
    const toggle = await screen.findByRole("switch", SWITCH);
    await waitFor(() => {
      expect(toggle).toBeChecked();
    });
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(lastSaved()).toBe(false);
    });
  });
});
