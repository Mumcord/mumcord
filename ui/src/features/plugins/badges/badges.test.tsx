import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createMockApi,
  setApi,
  type InstalledPlugin,
  type MockApi,
  type PluginNode,
} from "../../../api";
import { applyLanguage } from "../../../i18n";
import { MemberBadge } from "./MemberBadge";
import { PluginProfileSections, PROFILE_CARD_TIMEOUT_MS } from "./ProfileSections";

const text = (label: string): PluginNode[] => [
  {
    component: { type: "text", text: label, style: "body", color: null },
    animation: null,
    children: [],
  },
];

function plugin(id: string, extra: Partial<InstalledPlugin> = {}): InstalledPlugin {
  return {
    id,
    name: id,
    version: "1.0.0",
    enabled: true,
    status: { kind: "running" },
    granted: ["ui.member-badge", "ui.profile-card"],
    badge: false,
    ...extra,
  } as InstalledPlugin;
}

let api: MockApi;

function listPlugins(plugins: InstalledPlugin[]): void {
  api.pluginsList = () => Promise.resolve({ noPlugins: false, allOff: false, plugins });
}

function setTree(pluginId: string, kind: "memberBadge" | "profileCard", label: string): void {
  act(() => {
    api.mock.emit("pluginUi", {
      plugin: pluginId,
      change: { type: "setTree", slot: { kind, session: 7 }, nodes: text(label), serverId: null },
    });
  });
}

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("MemberBadge", () => {
  it("shows only the chosen plugin's badge", async () => {
    listPlugins([plugin("a", { badge: true }), plugin("b")]);
    render(<MemberBadge session={7} />);
    await waitFor(() => {
      setTree("b", "memberBadge", "from-b");
      setTree("a", "memberBadge", "from-a");
      expect(screen.getByText("from-a")).toBeTruthy();
    });
    expect(screen.queryByText("from-b")).toBeNull();
    expect(screen.getByRole("group", { name: "Badge from a" })).toBeTruthy();
  });

  it("shows nothing when no badge plugin is chosen", async () => {
    listPlugins([plugin("a")]);
    render(<MemberBadge session={7} />);
    setTree("a", "memberBadge", "from-a");
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.queryByText("from-a")).toBeNull();
  });

  it("renders 100 badges", async () => {
    listPlugins([plugin("a", { badge: true })]);
    render(
      <>
        {Array.from({ length: 100 }, (_, i) => (
          <MemberBadge key={i} session={7} />
        ))}
      </>,
    );
    await waitFor(() => {
      setTree("a", "memberBadge", "x");
      expect(screen.getAllByText("x")).toHaveLength(100);
    });
  });
});

describe("PluginProfileSections", () => {
  it("asks the plugins and shows an answer within 500 ms, ignores a stale or late one", async () => {
    listPlugins([plugin("fast"), plugin("late"), plugin("nogrant", { granted: [] })]);
    const asked: string[] = [];
    api.pluginUiEvent = (id) => {
      asked.push(id);
      return Promise.resolve(null);
    };
    setTree("late", "profileCard", "stale");
    render(<PluginProfileSections session={7} />);
    await waitFor(() => {
      expect(asked.sort()).toEqual(["fast", "late"]);
    });
    setTree("fast", "profileCard", "fast-section");
    expect(await screen.findByText("fast-section")).toBeTruthy();
    expect(screen.queryByText("stale")).toBeNull();

    const now = performance.now();
    vi.spyOn(performance, "now").mockReturnValue(now + PROFILE_CARD_TIMEOUT_MS + 100);
    setTree("late", "profileCard", "too-late");
    expect(screen.queryByText("too-late")).toBeNull();
    vi.restoreAllMocks();
  });
});
