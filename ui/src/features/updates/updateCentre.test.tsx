import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type MockApi, type UpdateStatus } from "../../api";
import { applyLanguage } from "../../i18n";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { NotificationBell, NotificationsHost, resetCentre } from "../notifications";
import { getCentreState } from "../notifications/centreStore";
import { Updater } from "./Updater";
import type { UpdateActions } from "./UpdateDialog";

const offer = { version: "0.4.5", url: "https://example.com/releases/v0.4.5", notes: [] };
let api: MockApi;

function fakeActions(): UpdateActions {
  return {
    download: vi.fn(() => Promise.resolve()),
    cancel: vi.fn(() => Promise.resolve()),
    restart: vi.fn(() => Promise.resolve()),
    skip: vi.fn(() => Promise.resolve()),
    openReleasePage: vi.fn(),
  };
}

async function setup(connected: boolean) {
  stubDialog();
  if (connected) {
    api = await connectedMock({ bindStore: true });
  } else {
    await applyLanguage("en");
    api = createMockApi();
    setApi(api);
  }
  const actions = fakeActions();
  render(
    <>
      <NotificationBell />
      <NotificationsHost overlayOpen={false} onOpenChat={vi.fn()} />
      <Updater actions={actions} />
    </>,
  );
  return actions;
}

async function emit(change: Partial<UpdateStatus>) {
  const status = { ...(await api.updateStatus()), ...change };
  act(() => {
    api.mock.emit("updateStatusChanged", { status, prompt: false });
  });
}

function updateEntries() {
  return getCentreState().items.filter((item) => item.kind === "update");
}

function openCentre() {
  fireEvent.click(screen.getByTestId("notification-bell"));
  return screen.getByRole("dialog", { name: "Notifications" });
}

afterEach(() => {
  resetCentre();
  resetAll();
});

describe("update entry in the notification centre (#168)", () => {
  it("appears once the update is ready, counts as unread and replaces an older version", async () => {
    await setup(false);
    await emit({ phase: "downloading", offer });
    expect(updateEntries()).toHaveLength(0);
    await emit({ phase: "ready", offer });
    expect(screen.getByTestId("notification-bell")).toHaveAccessibleName("Notifications, 1 unread");
    const centre = openCentre();
    expect(within(centre).getByText("Mumcord 0.4.5 is ready")).toBeInTheDocument();
    expect(within(centre).getByRole("button", { name: "What's new" })).toBeInTheDocument();
    await emit({ phase: "ready", offer: { ...offer, version: "0.4.6" } });
    expect(updateEntries()).toMatchObject([{ version: "0.4.6" }]);
  });

  it("is removed when the updater is no longer ready", async () => {
    await setup(false);
    await emit({ phase: "ready", offer });
    expect(updateEntries()).toHaveLength(1);
    await emit({ phase: "failed", offer });
    expect(updateEntries()).toHaveLength(0);
  });

  it("restarts right away when no server is connected", async () => {
    const actions = await setup(false);
    await emit({ phase: "ready", offer });
    const centre = openCentre();
    fireEvent.click(within(centre).getByRole("button", { name: "Restart to update" }));
    expect(actions.restart).toHaveBeenCalledOnce();
    expect(updateEntries()[0]?.read).toBe(true);
  });

  it("shows the ready dialog instead of restarting during a call", async () => {
    const actions = await setup(true);
    await emit({ phase: "ready", offer });
    const centre = openCentre();
    fireEvent.click(within(centre).getByRole("button", { name: "Restart to update" }));
    expect(actions.restart).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: "Mumcord 0.4.5 is ready" })).toBeInTheDocument();
    });
  });

  it("opens the release page for What's new", async () => {
    await setup(false);
    const openLink = vi.spyOn(api, "openLink");
    await emit({ phase: "ready", offer });
    const centre = openCentre();
    fireEvent.click(within(centre).getByRole("button", { name: "What's new" }));
    await waitFor(() => {
      expect(openLink).toHaveBeenCalledWith(offer.url);
    });
  });
});
