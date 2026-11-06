import { act, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { EngineError, type StageActivityView } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { mediaView, resetMediaView } from "../../media/mediaView";
import { resetActivityStore } from "../stage/activityStore";
import { PluginActivityLines } from "./ActivityLine";

const api = vi.hoisted(() => ({
  activities: [] as StageActivityView[],
  join: vi.fn<(activity: string) => Promise<null>>(() => Promise.resolve(null)),
  route: vi.fn(),
}));
vi.mock("../../../api", async (orig) => {
  const mocked = {
    stageActivities: () => Promise.resolve(api.activities),
    stageJoin: api.join,
    pluginsList: () =>
      Promise.resolve({ noPlugins: false, allOff: false, plugins: [], refused: [] }),
    on: () => () => undefined,
  };
  return { ...(await orig<typeof import("../../../api")>()), getApi: () => mocked };
});
vi.mock("../pluginRequestStore", () => ({ routePluginRequest: api.route }));

const STARTED: StageActivityView = {
  id: "a1",
  plugin: "brawl",
  pluginName: "Board Brawl",
  title: "Board Brawl",
  starter: 7,
  starterName: "Maya",
  participants: [7],
  max: 2,
  joined: false,
  startedByMe: false,
  web: false,
  kind: "activity",
};

async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function violations(container: Element): Promise<string[]> {
  const results = await axe.run(container);
  return results.violations.map((violation) => violation.id);
}

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api.join.mockReset();
  api.join.mockResolvedValue(null);
  api.route.mockClear();
  resetActivityStore();
  resetMediaView();
});

describe("activity line", () => {
  it("says who started what and joins on the user's click", async () => {
    api.activities = [STARTED];
    const { container } = render(<PluginActivityLines />);
    await settle();
    expect(screen.getByRole("list", { name: "Plugin activities" })).toBeInTheDocument();
    expect(screen.getByText("Maya started Board Brawl")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Join Board Brawl" }));
    await settle();
    expect(api.join).toHaveBeenCalledWith("a1");
    expect(screen.queryByText("This activity is full")).toBeNull();
    expect(await violations(container)).toEqual([]);
  });

  it.each([
    ["full", "This activity is full"],
    ["gone", "This activity has ended"],
    ["notPermitted", "You can't join Board Brawl right now"],
    ["failed", "Couldn't join Board Brawl"],
  ])("says why joining failed (%s)", async (reason, text) => {
    api.activities = [STARTED];
    api.join.mockRejectedValue(new EngineError("internal", reason));
    render(<PluginActivityLines />);
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "Join Board Brawl" }));
    expect(await screen.findByText(text)).toBeInTheDocument();
  });

  it("brings an activity the user is already in to the stage instead of joining again", async () => {
    api.activities = [{ ...STARTED, joined: true }];
    render(<PluginActivityLines />);
    await settle();
    expect(screen.queryByRole("button", { name: "Join Board Brawl" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open Board Brawl" }));
    expect(api.join).not.toHaveBeenCalled();
    expect(mediaView().cinema).toBe("plugin:brawl");
  });

  it("offers an official plugin to people without it and opens the install prompt", async () => {
    api.activities = [{ ...STARTED, kind: "getPlugin", joined: false }];
    const { container } = render(<PluginActivityLines />);
    await settle();
    expect(screen.getByText("Maya is playing Board Brawl")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Get Board Brawl" }));
    expect(api.route).toHaveBeenCalledWith({ kind: "install", plugin: "brawl" });
    expect(await violations(container)).toEqual([]);
  });

  it("shows no line for the user's own activity", async () => {
    api.activities = [{ ...STARTED, startedByMe: true, joined: true }];
    render(<PluginActivityLines />);
    await settle();
    expect(screen.queryByRole("list")).toBeNull();
  });
});
