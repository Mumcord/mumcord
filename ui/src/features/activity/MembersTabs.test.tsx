/**
 * The Members | Activity tab strip of the member column (FR-11.1, FR-11.2): switching by mouse and
 * by the arrow keys, Activity open by default, the choice remembered per server for the session, and
 * back to Activity when a new connection is synced. The shell hands the strip the members content, so the tests give it a
 * stand-in and drive the feed through the mock engine.
 */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { resetActivityStore } from "../plugins/stage/activityStore";
import { resetScreenShareStore } from "../screenshare/sharesStore";
import { resetWatchPartyStore } from "../watchparty/watchPartyStore";
import { resetActivityLogStore } from "./activityStore";
import { resetMembersTabStore } from "./membersTabStore";
import { MembersTabs } from "./MembersTabs";

const SERVER = 1;
const OTHER = 2;

describe("members column tabs", () => {
  let api: MockApi;

  beforeEach(async () => {
    // The saved history (#594) would carry events from one test into the next.
    localStorage.clear();
    resetActivityLogStore();
    resetMembersTabStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    api = await connectedMock();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetActivityLogStore();
    resetMembersTabStore();
    resetScreenShareStore();
    resetWatchPartyStore();
    resetActivityStore();
    resetAll();
  });

  it("opens on Activity and switches the panel by mouse, naming it by its tab (FR-11.1, FR-11.2)", () => {
    render(<MembersTabs serverId={SERVER} members={<p>member list</p>} />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Members", "Activity"]);
    expect(screen.getByRole("tablist")).toHaveAccessibleName("Members column");
    const membersTab = screen.getByRole("tab", { name: "Members" });
    const activityTab = screen.getByRole("tab", { name: "Activity" });
    expect(activityTab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("Activity");
    // Nothing happened yet on this connection (FR-11.14).
    expect(screen.getByText("No activity yet.")).toBeInTheDocument();

    fireEvent.click(membersTab);
    expect(membersTab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("Members");
    expect(screen.getByText("member list")).toBeInTheDocument();
  });

  it("switches with the arrow keys and moves the focus to the chosen tab (FR-11.1)", () => {
    render(<MembersTabs serverId={SERVER} members={<p>member list</p>} />);
    const membersTab = screen.getByRole("tab", { name: "Members" });
    const activityTab = screen.getByRole("tab", { name: "Activity" });

    fireEvent.keyDown(activityTab, { key: "ArrowLeft" });
    expect(membersTab).toHaveAttribute("aria-selected", "true");
    expect(document.activeElement).toBe(membersTab);

    fireEvent.keyDown(membersTab, { key: "ArrowRight" });
    expect(activityTab).toHaveAttribute("aria-selected", "true");
    expect(document.activeElement).toBe(activityTab);
    expect(screen.getByText("No activity yet.")).toBeInTheDocument();
  });

  it("goes back to Activity for a new connection (FR-11.2)", async () => {
    render(<MembersTabs serverId={SERVER} members={<p>member list</p>} />);
    fireEvent.click(screen.getByRole("tab", { name: "Members" }));
    expect(screen.getByText("member list")).toBeInTheDocument();

    // The same server, a new connection: its first sync starts a new log (FR-11.4).
    await act(async () => {
      await api.connect(SERVER, null, null);
    });
    expect(screen.getByRole("tab", { name: "Activity" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByText("member list")).not.toBeInTheDocument();
  });

  it("remembers the choice per server for the session (FR-11.2)", () => {
    render(<MembersTabs serverId={SERVER} members={<p>list one</p>} />);
    fireEvent.click(screen.getByRole("tab", { name: "Members" }));
    expect(screen.getByText("list one")).toBeInTheDocument();
    cleanup();

    // Another server keeps its own choice, which is still the default.
    render(<MembersTabs serverId={OTHER} members={<p>list two</p>} />);
    expect(screen.getByRole("tab", { name: "Activity" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByText("list two")).not.toBeInTheDocument();
    cleanup();

    render(<MembersTabs serverId={SERVER} members={<p>list one</p>} />);
    expect(screen.getByRole("tab", { name: "Members" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("list one")).toBeInTheDocument();
  });
});
