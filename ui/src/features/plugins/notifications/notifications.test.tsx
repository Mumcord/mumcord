import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getApi, setApi, type MockApi } from "../../../api";
import { axeViolations } from "../../../test/axe";
import { connectedMock } from "../../channels/testUtils";
import { resetRequestsStore } from "../../requests/requestsStore";
import { NotificationBell, NotificationsHost, resetNotifications } from "../../notifications";
import { getCentreState, resetCentre } from "../../notifications/centreStore";
import { subscribePluginRequests, type PluginRequest } from "./pluginRequests";

let api: MockApi;

async function setup() {
  resetNotifications();
  resetCentre();
  resetRequestsStore();
  api = await connectedMock();
  return render(
    <>
      <NotificationBell />
      <main id="chat" tabIndex={-1} />
      <NotificationsHost overlayOpen={false} onOpenChat={vi.fn()} />
    </>,
  );
}

function openPanel() {
  fireEvent.click(screen.getByTestId("notification-bell"));
  return screen.getByRole("dialog", { name: "Notifications" });
}

const SUGGESTION = {
  serverId: 1,
  plugin: "dice",
  name: "Dice",
  reason: "<b>Great</b> for game night",
  seq: "3",
  source: { kind: "official" as const, version: "1.0.0" },
};

afterEach(() => {
  resetNotifications();
  resetCentre();
  resetRequestsStore();
  setApi(undefined);
});

describe("plugin entries of the notification centre", () => {
  it("shows a crash shut-off under Plugins and opens the plugin settings", async () => {
    await setup();
    const requests: PluginRequest[] = [];
    const stop = subscribePluginRequests((r) => requests.push(r));
    act(() => {
      api.mock.emit("pluginsNotification", { plugin: "dice", notice: { kind: "autoOff" } });
    });
    const centre = openPanel();
    const entry = await within(centre).findByRole("button", { name: /^dice was turned off/ });
    expect(entry).toHaveTextContent("Turned off after repeated crashes");
    expect(within(centre).getByTestId("centre-filter-plugins")).toHaveTextContent("Plugins, 1");
    fireEvent.click(entry);
    expect(requests).toEqual([{ kind: "installed", plugin: "dice" }]);
    stop();
  });

  it("shows a plugin message as plain text", async () => {
    await setup();
    act(() => {
      api.mock.emit("pluginsNotification", {
        plugin: "dice",
        notice: { kind: "message", title: "<i>Rolled</i>", body: "6 and 6" },
      });
    });
    const centre = openPanel();
    expect(await within(centre).findByText("<i>Rolled</i>")).toBeInTheDocument();
    expect(within(centre).getByText("6 and 6")).toBeInTheDocument();
  });

  it("tells about an available update and opens the Updates tab", async () => {
    await setup();
    const requests: PluginRequest[] = [];
    const stop = subscribePluginRequests((r) => requests.push(r));
    act(() => {
      api.mock.emit("pluginsNotification", {
        plugin: "dice",
        notice: { kind: "updateAvailable", name: "Dice", version: "1.2.0" },
      });
    });
    const centre = openPanel();
    fireEvent.click(await within(centre).findByTestId("centre-plugin-updates"));
    expect(requests).toEqual([{ kind: "updates" }]);
    stop();
  });

  it("offers Install, Details, Not now and Don't ask again for a server suggestion", async () => {
    const view = await setup();
    const requests: PluginRequest[] = [];
    const stop = subscribePluginRequests((r) => requests.push(r));
    act(() => {
      api.mock.emit("pluginsSuggestion", SUGGESTION);
    });
    const centre = openPanel();
    expect(await within(centre).findByText("<b>Great</b> for game night")).toBeInTheDocument();
    expect(await axeViolations(view.container)).toEqual([]);
    fireEvent.click(within(centre).getByTestId("centre-plugin-details"));
    expect(requests).toEqual([{ kind: "details", plugin: "dice" }]);
    stop();
  });

  it("keeps the same list read and replaces it when the list number changes", async () => {
    await setup();
    act(() => {
      api.mock.emit("pluginsSuggestion", SUGGESTION);
    });
    const [first] = getCentreState().items;
    act(() => {
      api.mock.emit("pluginsSuggestion", SUGGESTION);
    });
    expect(getCentreState().items).toHaveLength(1);
    expect(getCentreState().items[0]).toBe(first);
    act(() => {
      api.mock.emit("pluginsSuggestion", { ...SUGGESTION, seq: "4" });
    });
    expect(getCentreState().items).toHaveLength(1);
    expect(getCentreState().items[0]).not.toBe(first);
  });

  it("Not now hides the entry after the host stored it", async () => {
    await setup();
    const notNow = vi.spyOn(getApi(), "pluginSuggestionNotNow");
    act(() => {
      api.mock.emit("pluginsSuggestion", SUGGESTION);
    });
    const centre = openPanel();
    fireEvent.click(await within(centre).findByTestId("centre-plugin-not-now"));
    await waitFor(() => {
      expect(getCentreState().items).toEqual([]);
    });
    expect(notNow).toHaveBeenCalledWith(1, "dice");
  });

  it("Don't ask again removes every suggestion of that server", async () => {
    await setup();
    const dismiss = vi.spyOn(getApi(), "pluginSuggestionsDismissServer");
    act(() => {
      api.mock.emit("pluginsSuggestion", SUGGESTION);
      api.mock.emit("pluginsSuggestion", { ...SUGGESTION, plugin: "poll", name: "Poll" });
    });
    const centre = openPanel();
    fireEvent.click(
      (await within(centre).findAllByTestId("centre-plugin-dont-ask"))[0] as HTMLElement,
    );
    await waitFor(() => {
      expect(getCentreState().items).toEqual([]);
    });
    expect(dismiss).toHaveBeenCalledWith(1);
  });

  it("opens the Add repository dialog with the announced key for a third-party plugin", async () => {
    await setup();
    const requests: PluginRequest[] = [];
    const stop = subscribePluginRequests((r) => requests.push(r));
    act(() => {
      api.mock.emit("pluginsSuggestion", {
        ...SUGGESTION,
        plugin: "other",
        name: "other",
        source: { kind: "repository", url: "https://example.org/repo", key: "abc" },
      });
    });
    const centre = openPanel();
    fireEvent.click(await within(centre).findByTestId("centre-plugin-add-repository"));
    expect(requests).toEqual([
      { kind: "addRepository", url: "https://example.org/repo", key: "abc" },
    ]);
    stop();
  });
});
