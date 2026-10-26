import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import type { UserStats } from "../../bindings/commands";
import { PreferencesContext, type Preferences } from "../../app/preferences";
import { ChannelsPanel } from "../channels/ChannelsPanel";
import { connectedMock, resetAll, stubDialog } from "../channels/testUtils";
import { ProfileCard } from "../members/ProfileCard";
import { ConnectionQuality } from "./ConnectionQuality";
import { UserStatsWatcher, WATCH_DEBOUNCE_MS } from "./UserStatsWatcher";
import { requestConnectionExpanded, takeConnectionExpandedRequest } from "./connectionExpanded";
import { connectionLevel, isWeakConnection, resetUserStatsStore } from "./userStatsStore";

const ANNA = 2;

const none: UserStats = {
  pingMs: null,
  transport: null,
  lossPercent: null,
  latePercent: null,
  bandwidthKbps: null,
  clientRelease: null,
  clientVersion: null,
  os: null,
  osVersion: null,
};

let api: MockApi;

beforeEach(stubDialog);
afterEach(() => {
  vi.useRealTimers();
  resetUserStatsStore();
  takeConnectionExpandedRequest();
  resetAll();
});

function report(stats: Partial<UserStats>, session = ANNA) {
  act(() => {
    api.mock.emit("userStatsChanged", { serverId: 1, session, stats: { ...none, ...stats } });
  });
}

describe("isWeakConnection", () => {
  it("is weak above 250 ms or 5 % loss, never for unknown values", () => {
    expect(isWeakConnection(null)).toBe(false);
    expect(isWeakConnection(none)).toBe(false);
    expect(isWeakConnection({ ...none, pingMs: 250 })).toBe(false);
    expect(isWeakConnection({ ...none, pingMs: 251 })).toBe(true);
    expect(isWeakConnection({ ...none, lossPercent: 5 })).toBe(false);
    expect(isWeakConnection({ ...none, lossPercent: 5.1 })).toBe(true);
  });
});

describe("weak connection icon", () => {
  const treeRow = () => screen.getByRole("treeitem", { name: /^Anna/ });

  beforeEach(async () => {
    api = await connectedMock();
  });

  it("appears for a weak connection with a tooltip and goes when it recovers", async () => {
    render(<ChannelsPanel />);
    await screen.findByRole("tree", { name: "Channels" });
    report({ pingMs: 40 });
    expect(treeRow().querySelector('[data-icon="signal"]')).toBeNull();
    report({ pingMs: 400 });
    expect(within(treeRow()).getByRole("img", { name: /Weak connection/ })).toHaveAttribute(
      "data-tooltip",
      expect.stringContaining("Weak connection"),
    );
    report({ pingMs: 40, lossPercent: 9 });
    expect(treeRow().querySelector('[data-icon="signal"]')).not.toBeNull();
    report({ pingMs: 40, lossPercent: 1 });
    expect(treeRow().querySelector('[data-icon="signal"]')).toBeNull();
  });

  it("is hidden by the switch", async () => {
    const off = { showWeakConnectionIcon: false } as Preferences;
    render(
      <PreferencesContext value={off}>
        <ChannelsPanel />
      </PreferencesContext>,
    );
    await screen.findByRole("tree", { name: "Channels" });
    report({ pingMs: 900 });
    expect(treeRow().querySelector('[data-icon="signal"]')).toBeNull();
  });
});

describe("profile card figures", () => {
  beforeEach(async () => {
    api = await connectedMock();
  });

  it("lists the returned fields and hides the missing ones", async () => {
    render(<ProfileCard session={ANNA} onClose={() => undefined} />);
    await screen.findByRole("heading", { name: /Anna/ });
    expect(screen.getByRole("button", { name: "Connection: Unknown" })).toBeInTheDocument();
    report({ pingMs: 41.6, lossPercent: 1.25, transport: "udp" });
    fireEvent.click(await screen.findByRole("button", { name: "Connection: Good" }));
    const list = await screen.findByLabelText("Connection");
    expect(list).toHaveTextContent("Ping42 ms");
    expect(list).toHaveTextContent("Packet loss1.3 %");
    expect(list).toHaveTextContent("Voice transportUDP");
    expect(list).not.toHaveTextContent("Client");
    expect(list).not.toHaveTextContent("Bandwidth");
  });

  it("has a hidden chevron inside the toggle whose state follows aria-expanded", async () => {
    render(<ConnectionQuality session={ANNA} />);
    report({ pingMs: 10 });
    const toggle = await screen.findByRole("button", { name: /^Connection:/ });
    const chevron = toggle.querySelector(".connection-quality__chevron");
    expect(chevron).toHaveAttribute("aria-hidden", "true");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("starts collapsed again on every opening, unless expansion was requested once", async () => {
    const first = render(<ConnectionQuality session={ANNA} />);
    report({ pingMs: 10 });
    fireEvent.click(await screen.findByRole("button", { name: /^Connection:/ }));
    first.unmount();
    const second = render(<ConnectionQuality session={ANNA} />);
    expect(await screen.findByRole("button", { name: /^Connection:/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    second.unmount();
    requestConnectionExpanded();
    const third = render(<ConnectionQuality session={ANNA} />);
    expect(await screen.findByRole("button", { name: /^Connection:/ })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    third.unmount();
    render(<ConnectionQuality session={ANNA} />);
    expect(await screen.findByRole("button", { name: /^Connection:/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("shows client and OS as capped plain text", async () => {
    render(<ConnectionQuality session={ANNA} />);
    report({
      clientRelease: `<b>Mumcord 1.2.0</b>${"x".repeat(100)}`,
      os: "Linux",
      osVersion: "6.1",
    });
    fireEvent.click(await screen.findByRole("button", { name: /^Connection:/ }));
    const list = await screen.findByLabelText("Connection");
    expect(list).toHaveTextContent("Operating systemLinux 6.1");
    expect(list.querySelector("b")).toBeNull();
    const client = within(list).getByText(/^<b>Mumcord/);
    expect(client.textContent.length).toBe(64);
  });

  it("is cleared when the person leaves", async () => {
    render(<ConnectionQuality session={ANNA} />);
    report({ pingMs: 10 });
    fireEvent.click(await screen.findByRole("button", { name: "Connection: Good" }));
    expect(await screen.findByLabelText("Connection")).toBeInTheDocument();
    act(() => {
      api.mock.emit("userRemoved", {
        serverId: 1,
        session: ANNA,
      });
    });
    expect(screen.queryByLabelText("Connection")).toBeNull();
  });
});

describe("connection section", () => {
  beforeEach(async () => {
    api = await connectedMock();
  });

  it("derives the level from the weak thresholds, fair at half", () => {
    expect(connectionLevel(null)).toBe("unknown");
    expect(connectionLevel({ ...none, pingMs: 125 })).toBe("good");
    expect(connectionLevel({ ...none, pingMs: 126 })).toBe("fair");
    expect(connectionLevel({ ...none, lossPercent: 2.6 })).toBe("fair");
    expect(connectionLevel({ ...none, pingMs: 251 })).toBe("weak");
    expect(connectionLevel({ ...none, lossPercent: 5.1 })).toBe("weak");
  });

  it("counts a missing ping as unknown, not as good", () => {
    expect(connectionLevel(none)).toBe("unknown");
    expect(connectionLevel({ ...none, transport: "tcp", clientRelease: "Mumble 1.5.0" })).toBe(
      "unknown",
    );
  });

  it("hides a ping the server did not send", async () => {
    render(<ConnectionQuality session={ANNA} />);
    report({ transport: "tcp", clientRelease: "Mumble 1.5.0" });
    fireEvent.click(await screen.findByRole("button", { name: "Connection: Unknown" }));
    const list = await screen.findByLabelText("Connection");
    expect(list).not.toHaveTextContent("Ping");
    expect(list).not.toHaveTextContent("0 ms");
  });

  it("is collapsed by default and toggles by click, not remembered", async () => {
    const view = render(<ConnectionQuality session={ANNA} />);
    report({ pingMs: 300 });
    const toggle = await screen.findByRole("button", { name: "Connection: Weak" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText("Connection")).toBeNull();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByLabelText("Connection")).toHaveTextContent("Ping300 ms");
    view.unmount();
    render(<ConnectionQuality session={ANNA} />);
    expect(screen.getByRole("button", { name: "Connection: Weak" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });
});

describe("UserStatsWatcher", () => {
  it("reports the visible people after a short wait", async () => {
    api = await connectedMock();
    const watch = vi.spyOn(api, "userStatsWatch");
    render(<UserStatsWatcher />);
    expect(watch).not.toHaveBeenCalled();
    await waitFor(
      () => {
        expect(watch).toHaveBeenCalledTimes(1);
      },
      {
        timeout: WATCH_DEBOUNCE_MS * 4,
      },
    );
    const [serverId, sessions] = watch.mock.calls[0] ?? [];
    expect(serverId).toBe(1);
    expect(sessions).toContain(ANNA);
  });
});
