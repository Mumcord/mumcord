import { act, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginNode, StageActivityView } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { StageShown } from "../../media/stageContext";
import { resetStageEntries, useStageEntries } from "../../media/stageRegistry";
import { resetMediaView } from "../../media/mediaView";
import { resetActivityStore } from "./activityStore";
import { FULLSCREEN_BANNER_MS, PluginStage } from "./PluginStage";
import { PluginStageHost } from "./PluginStageHost";

const api = vi.hoisted(() => ({
  activities: [] as StageActivityView[],
  plugins: [] as { id: string; name: string; source: unknown; icon: null }[],
  listeners: new Map<string, (payload: unknown) => void>(),
  visibility: vi.fn<(plugin: string, visible: boolean) => Promise<null>>(() =>
    Promise.resolve(null),
  ),
  leave: vi.fn<(activity: string) => Promise<null>>(() => Promise.resolve(null)),
  action: vi.fn<(...args: unknown[]) => Promise<null>>(() => Promise.resolve(null)),
}));
vi.mock("../../../api", async (orig) => {
  const mocked = {
    stageActivities: () => Promise.resolve(api.activities),
    stageLeave: api.leave,
    stageVisibility: api.visibility,
    pluginUiAction: api.action,
    pluginUiEvent: () => Promise.resolve(null),
    pluginFramesMute: () => Promise.resolve(null),
    pluginsUiTrees: () => Promise.resolve([]),
    pluginsList: () => Promise.resolve({ noPlugins: false, allOff: false, plugins: api.plugins }),
    on: (name: string, handler: (payload: unknown) => void) => {
      const key = `${name}:${String(Math.random())}`;
      api.listeners.set(key, handler);
      return () => {
        api.listeners.delete(key);
      };
    },
  };
  return { ...(await orig<typeof import("../../../api")>()), getApi: () => mocked };
});

vi.mock("../../channels/serverStore", () => ({
  useVoiceServerId: () => 1,
  useServerStateOf: () => ({}),
  selfUser: () => ({ selfDeaf: false, serverDeaf: false }),
}));

function emit(name: string, payload: unknown) {
  act(() => {
    for (const [key, handler] of api.listeners) if (key.startsWith(`${name}:`)) handler(payload);
  });
}

const BRAWL: StageActivityView = {
  id: "a1",
  plugin: "brawl",
  pluginName: "Board Brawl",
  title: "Board Brawl",
  starter: 7,
  starterName: "Maya",
  participants: [7, 8, 9],
  max: 4,
  joined: true,
  startedByMe: false,
  web: false,
  kind: "activity",
};

const MEEPLE = { kind: "repo", repositoryId: 1, name: "Meeple Works" };

async function violations(container: Element): Promise<string[]> {
  const results = await axe.run(container, { iframes: false });
  return results.violations.map((violation) => violation.id);
}

/** Lets the plugin list and the activity list arrive. */
async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api.activities = [];
  api.plugins = [{ id: "brawl", name: "Board Brawl", source: MEEPLE, icon: null }];
  api.visibility.mockClear();
  api.leave.mockClear();
  api.action.mockClear();
  resetActivityStore();
  resetStageEntries();
  resetMediaView();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("stage header", () => {
  it("says who started it, how many are in, who made it and offers Full screen and Leave", async () => {
    const { container } = render(<PluginStage activity={BRAWL} />);
    await settle();
    expect(screen.getByRole("region", { name: "Media area: Board Brawl" })).toBeInTheDocument();
    expect(
      screen.getByText("Started by Maya · 3 in the game · plugin by Meeple Works"),
    ).toBeInTheDocument();
    expect(screen.getByText("Unofficial")).toBeInTheDocument();
    expect(screen.getByText("Mumble connection · slower")).toBeInTheDocument();
    const bar = screen.getByRole("toolbar", { name: "Board Brawl controls" });
    expect(bar).toContainElement(screen.getByRole("button", { name: "Full screen" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave Board Brawl" }));
    expect(api.leave).toHaveBeenCalledWith("a1");
    expect(await violations(container)).toEqual([]);
  });

  it("says 'you' for the starter and has no chip or maker for official plugins", async () => {
    api.plugins = [{ id: "brawl", name: "Board Brawl", source: { kind: "official" }, icon: null }];
    render(<PluginStage activity={{ ...BRAWL, startedByMe: true, participants: [7] }} />);
    await settle();
    expect(screen.getByText("Started by you · 1 in the game")).toBeInTheDocument();
    expect(screen.queryByText("Unofficial")).toBeNull();
  });
});

describe("stage content", () => {
  const button: PluginNode = {
    component: {
      type: "button",
      label: "Offer a draw",
      action: "draw",
      kind: "primary",
      disabled: false,
      expanded: null,
      detail: null,
    },
    animation: null,
    children: [],
  };

  it("draws the stage tree and sends its clicks for the stage slot", async () => {
    render(<PluginStage activity={BRAWL} />);
    await settle();
    emit("pluginUi", { plugin: "brawl", change: { type: "stageTree", nodes: [button] } });
    fireEvent.click(screen.getByRole("button", { name: "Offer a draw" }));
    expect(api.action).toHaveBeenCalledWith(
      "brawl",
      { kind: "stage" },
      "draw",
      { type: "none" },
      null,
    );
  });

  it("shows the plugin's web page for a web activity", async () => {
    const { container } = render(<PluginStage activity={{ ...BRAWL, web: true }} />);
    await settle();
    emit("pluginUi", {
      plugin: "brawl",
      change: { type: "webviewShow", place: "stage", url: "mumcord-web://localhost/brawl/a.html" },
    });
    expect(container.querySelector('.plugin-frame[data-place="stage"] iframe')).not.toBeNull();
  });
});

describe("full screen", () => {
  let element: Element | null = null;

  beforeEach(() => {
    element = null;
    Object.defineProperty(document, "fullscreenElement", {
      configurable: true,
      get: () => element,
    });
    // The stage is the only element that asks for full screen here.
    HTMLElement.prototype.requestFullscreen = () => {
      element = document.querySelector(".plugin-stage");
      document.dispatchEvent(new Event("fullscreenchange"));
      return Promise.resolve();
    };
    document.exitFullscreen = () => {
      element = null;
      document.dispatchEvent(new Event("fullscreenchange"));
      return Promise.resolve();
    };
  });

  it("starts from the button, shows the banner for 3 s and Esc ends it", async () => {
    render(<PluginStage activity={BRAWL} />);
    await settle();
    vi.useFakeTimers();
    act(() => {
      screen.getByRole("button", { name: "Full screen" }).click();
    });
    const region = screen.getByRole("region", { name: "Media area: Board Brawl" });
    expect(element).toBe(region);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Board Brawl is full screen · Esc to exit",
    );
    act(() => {
      vi.advanceTimersByTime(FULLSCREEN_BANNER_MS);
    });
    expect(screen.queryByRole("status")).toBeNull();
    act(() => {
      fireEvent.keyDown(document, { key: "Escape" });
    });
    expect(element).toBeNull();
    expect(screen.getByRole("button", { name: "Full screen" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });
});

describe("stage visibility", () => {
  it("tells the plugin when its tab is shown or hidden, and hidden once it goes", async () => {
    const view = (shown: boolean) => (
      <StageShown.Provider value={shown}>
        <PluginStage activity={BRAWL} />
      </StageShown.Provider>
    );
    const { rerender, unmount } = render(view(true));
    await settle();
    expect(api.visibility).toHaveBeenLastCalledWith("brawl", true);
    rerender(view(false));
    expect(api.visibility).toHaveBeenLastCalledWith("brawl", false);
    rerender(view(true));
    expect(api.visibility).toHaveBeenLastCalledWith("brawl", true);
    unmount();
    expect(api.visibility).toHaveBeenLastCalledWith("brawl", false);
  });
});

describe("PluginStageHost", () => {
  function Tabs() {
    const { plugins } = useStageEntries();
    return [...plugins.entries()].map(([id, item]) => (
      <div key={id} data-testid={id}>
        {item.label}
      </div>
    ));
  }

  it("puts joined activities into the media area and takes them out once left", async () => {
    api.activities = [BRAWL, { ...BRAWL, id: "a2", plugin: "dice", joined: false }];
    render(
      <>
        <PluginStageHost />
        <Tabs />
      </>,
    );
    await settle();
    expect(screen.getByTestId("plugin:brawl")).toHaveTextContent("Board Brawl");
    expect(screen.queryByTestId("plugin:dice")).toBeNull();
    api.activities = [];
    emit("pluginsStageChanged", {});
    await settle();
    expect(screen.queryByTestId("plugin:brawl")).toBeNull();
  });

  it("shows the limit a plugin's own start ran into", async () => {
    const { PluginToasts } = await import("../prompt/PluginToasts");
    render(
      <>
        <PluginStageHost />
        <PluginToasts />
      </>,
    );
    emit("pluginsStageRefused", { plugin: "brawl", reason: "tooMany" });
    expect(await screen.findByText("Too many activities in this channel")).toBeInTheDocument();
    emit("pluginsStageRefused", { plugin: "brawl", reason: "full" });
    expect(await screen.findByText("This activity is full")).toBeInTheDocument();
  });
});
