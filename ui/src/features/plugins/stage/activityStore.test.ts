/**
 * The non-React face of the plugin activity store (T3): the activity log follows the activities
 * without a mounted component, so its listener must fire from the host events alone, the getter
 * must read the same list the hooks read, and the first listener must trigger the load.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi, StageActivityView } from "../../../api";
import { connectedMock, resetAll } from "../../channels/testUtils";
import { resetActivityStore, stageActivities, subscribeStageActivities } from "./activityStore";

const ANNA = 2;

function activity(patch: Partial<StageActivityView> = {}): StageActivityView {
  return {
    id: "a1",
    plugin: "brawl",
    pluginName: "Board Brawl",
    title: "Board Brawl",
    starter: ANNA,
    starterName: "Anna",
    participants: [ANNA],
    max: 4,
    joined: false,
    startedByMe: false,
    web: false,
    kind: "activity",
    ...patch,
  };
}

describe("subscribeStageActivities", () => {
  let api: MockApi;

  beforeEach(async () => {
    resetActivityStore();
    api = await connectedMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetActivityStore();
    resetAll();
  });

  it("loads for the first listener, fires on a change, and reads the current list", async () => {
    const started = activity();
    const stopped = activity({ id: "a2", title: "Stopped" });
    const listed = vi.spyOn(api, "stageActivities").mockResolvedValue([started]);

    const listener = vi.fn();
    const unsubscribe = subscribeStageActivities(listener);
    await vi.waitFor(() => {
      expect(listener).toHaveBeenCalledOnce();
    });
    expect(stageActivities()).toEqual([started]);

    listed.mockResolvedValue([started, stopped]);
    api.mock.emit("pluginsStageChanged", {});
    await vi.waitFor(() => {
      expect(stageActivities()).toEqual([started, stopped]);
    });
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
  });
});
