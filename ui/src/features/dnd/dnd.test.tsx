import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type DndState, type MockApi } from "../../api";
import { PreferencesProvider } from "../../app/PreferencesProvider";
import { applyLanguage } from "../../i18n";
import { DndGroup } from "../settings/sections/DndGroup";
import { QuickSettings } from "../userpanel/QuickSettings";
import { DndIndicator } from "./DndIndicator";
import { dndSource } from "./useDnd";

let api: MockApi;

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

const state = (manual: boolean, followSystem: boolean, on: boolean | null): DndState => ({
  manual,
  followSystem,
  system: { desktop: on === null ? null : "KDE Plasma", on },
});

describe("dndSource", () => {
  it("names the switch first, then the followed system state", () => {
    expect(dndSource(null)).toBeNull();
    expect(dndSource(state(false, true, false))).toBeNull();
    expect(dndSource(state(false, true, true))).toBe("system");
    expect(dndSource(state(false, false, true))).toBeNull();
    expect(dndSource(state(true, true, true))).toBe("manual");
    expect(dndSource(state(true, false, null))).toBe("manual");
  });
});

describe("DndIndicator", () => {
  it("shows nothing while off, the manual source and then the system source live", async () => {
    const { container } = render(<DndIndicator />);
    await act(async () => {
      await api.dndState();
    });
    expect(container).toBeEmptyDOMElement();

    await act(async () => {
      await api.dndSet(true);
    });
    expect(screen.getByRole("status")).toHaveTextContent("Do Not Disturb · manual");

    await act(async () => {
      await api.dndSet(false);
      api.mock.setSystemDnd({ desktop: "KDE Plasma", on: true });
    });
    expect(screen.getByRole("status")).toHaveTextContent("Do Not Disturb (system)");
    expect(
      screen.getByText("Turn it off in your desktop's notification settings."),
    ).toBeInTheDocument();
  });
});

describe("DndGroup", () => {
  it("switches Do Not Disturb and following the system, and names the detected desktop", async () => {
    render(<DndGroup />);
    const toggle = await screen.findByRole("switch", { name: "Do Not Disturb" });
    const follow = screen.getByRole("switch", { name: "Follow system Do Not Disturb" });
    expect(toggle).not.toBeChecked();
    expect(follow).toBeChecked();
    expect(screen.getByText("Detected")).toBeInTheDocument();
    expect(
      screen.getByText("KDE Plasma · Do Not Disturb is off. Changes are picked up live."),
    ).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(await screen.findByRole("switch", { name: "Do Not Disturb" })).toBeChecked();
    expect((await api.dndState()).manual).toBe(true);

    fireEvent.click(follow);
    await act(async () => {
      await api.dndState();
    });
    expect((await api.dndState()).followSystem).toBe(false);
  });

  it("says so when the system state can't be read", async () => {
    api.mock.setSystemDnd({ desktop: null, on: null });
    render(<DndGroup />);
    expect(await screen.findByText("Unavailable")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Mumcord can't read your system's Do Not Disturb state here. Only its own switch works.",
      ),
    ).toBeInTheDocument();
  });
});

describe("QuickSettings", () => {
  it("offers Do Not Disturb first and turns it on", async () => {
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
    const rows = await screen.findAllByRole("menuitemcheckbox");
    expect(rows[0]).toHaveAccessibleName(/^Do Not Disturb/);
    expect(rows[0]).toHaveAttribute("aria-checked", "false");
    fireEvent.click(rows[0] as HTMLElement);
    expect(
      await screen.findByRole("menuitemcheckbox", { name: /^Do Not Disturb/ }),
    ).toHaveAttribute("aria-checked", "true");
  });
});

describe("message notifications during Do Not Disturb", () => {
  it("play no sound and show nothing", async () => {
    await api.dndSet(true);
    const outcome = await api.notifyMessage({
      serverId: 1,
      conversation: { kind: "channel", channelId: 1 },
      onScreen: false,
      summary: "#Raid",
      body: "Alice: @me look",
      actionLabel: "Open chat",
      mention: true,
    });
    expect(outcome).toBe("none");
    expect(api.mock.messageSounds()).toEqual([]);
    expect(api.mock.desktopNotices()).toEqual([]);
  });
});
