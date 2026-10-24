import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type User } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { AvatarStack } from "./AvatarStack";
import { ChannelTree } from "./ChannelTree";
import { getServerState, type ServerState } from "./serverStore";
import { resetAll } from "./testUtils";
import { stackFit, stackPeople } from "./tree";

const NAMES = "Ana Ben Cara Dan Eve Finn Gus Hana Ivo Jade Kai Lou Mia Ned".split(" ");

function user(session: number, name: string, channelId: number): User {
  return {
    session,
    channelId,
    name,
    certHash: null,
    registered: false,
    selfMute: false,
    selfDeaf: false,
    serverMute: false,
    serverDeaf: false,
    suppressed: false,
    listeningChannelIds: [],
    hasAvatar: false,
    avatarHash: null,
    hasComment: false,
    volume: 100,
    localMute: false,
  };
}

let base: ServerState;

/** The seeded server with "Lobby" (3 people) and "Raid" (the given number of people). */
function serverState(crowd: number, talking: number[] = []): ServerState {
  const people = [
    ...["Zed", "Yan", "Xia"].map((n, i) => user(100 + i, n, 1)),
    ...NAMES.slice(0, crowd).map((n, i) => user(i + 1, n, 2)),
  ];
  return {
    ...base,
    selfSession: null,
    users: new Map(people.map((u) => [u.session, u])),
    talking: new Set(talking),
  };
}

const noop = () => undefined;
const renderTree = (state: ServerState) =>
  render(
    <ChannelTree
      state={state}
      selectedChannelId={null}
      onSelect={noop}
      onJoin={noop}
      onOpenPerson={noop}
    />,
  );
const item = (name: RegExp) => screen.getByRole("treeitem", { name });

beforeEach(async () => {
  await applyLanguage("en");
  const api = createMockApi();
  setApi(api);
  getServerState(); // binds the store before the snapshot arrives
  await api.connect(1, null);
  base = getServerState();
});
afterEach(resetAll);

describe("stackPeople", () => {
  it("keeps the member order and moves talking people to the front", () => {
    const members = NAMES.slice(0, 8).map((n, i) => user(i + 1, n, 2));
    expect(stackPeople(members, new Set()).map((u) => u.name)).toEqual(NAMES.slice(0, 5));
    expect(stackPeople(members, new Set([7, 3])).map((u) => u.name)).toEqual([
      "Cara",
      "Gus",
      "Ana",
      "Ben",
      "Dan",
    ]);
  });
});

describe("stackFit", () => {
  // At the 15rem default a top-level stack line has a 12rem content box (1.5rem indent).
  it("shows five avatars and the chip on a full line at the default width", () => {
    expect(stackFit(12, 9)).toBe(5);
    expect(stackFit(12, 3)).toBe(3);
  });

  it("leaves room for the Show all toggle and the chip on narrower lines", () => {
    // 12rem minus a ~6.5rem toggle and the gap.
    expect(stackFit(5, 14)).toBe(3);
    // Without the chip one more fits.
    expect(stackFit(5, 4)).toBe(4);
  });

  it("maps width to count in avatar steps, up to five", () => {
    expect([2, 3, 4, 5, 6, 7, 20].map((rem) => stackFit(rem, 20))).toEqual([1, 1, 2, 3, 4, 5, 5]);
    // When everyone fits, no room is kept for the chip.
    const exact: [number, number][] = [
      [1.5, 1],
      [2.5, 2],
      [3.5, 3],
      [4.5, 4],
      [5.5, 5],
    ];
    expect(exact.map(([rem, total]) => stackFit(rem, total))).toEqual([1, 2, 3, 4, 5]);
  });

  it("never shows fewer than one avatar", () => {
    expect([0, 1].map((rem) => stackFit(rem, 5))).toEqual([1, 1]);
  });
});

describe("AvatarStack", () => {
  const people = (n: number, talking: number[] = []) =>
    NAMES.slice(0, n).map((name, i) => ({
      user: user(i + 1, name, 2),
      talking: talking.includes(i + 1),
      src: null,
    }));

  it("shows the given avatars and a +N chip for the rest", () => {
    const { container } = render(<AvatarStack people={people(5)} total={23} />);
    expect(container.querySelectorAll(".avatar")).toHaveLength(5);
    expect(container.querySelector(".avatar-stack__more")).toHaveAttribute("data-more", "+18");
    expect(screen.getByRole("img")).toHaveAccessibleName(
      "5 of 23 people: Ana, Ben, Cara, Dan, Eve and 18 more",
    );
  });

  it("has no chip when everyone is shown and announces talking people", () => {
    const { container } = render(<AvatarStack people={people(2, [2])} total={2} />);
    expect(container.querySelector(".avatar-stack__more")).toBeNull();
    expect(container.querySelector(".avatar[data-talking='true']")).not.toBeNull();
    expect(screen.getByRole("img")).toHaveAccessibleName("2 people: Ana, Ben (talking)");
  });

  it("badges muted, deafened and locally muted avatars and names their state", () => {
    const [ana, ben, cara, dan, eve] = people(5);
    if (!ana || !ben || !cara || !dan || !eve) throw new Error("people(5) has five people");
    const stacked = [
      { ...ana, user: { ...ana.user, selfMute: true } },
      { ...ben, user: { ...ben.user, serverMute: true, localMute: true } },
      { ...cara, user: { ...cara.user, selfDeaf: true, localMute: true } },
      { ...dan, user: { ...dan.user, suppressed: true } },
      eve,
    ];
    const { container } = render(<AvatarStack people={stacked} total={5} />);
    const badges = Array.from(container.querySelectorAll(".avatar-stack__badge"));
    expect(badges.map((b) => b.getAttribute("data-state"))).toEqual([
      "self",
      "serverMuted",
      "localMuted",
      "suppressed",
    ]);
    expect(badges.map((b) => b.querySelector("svg")?.getAttribute("data-icon"))).toEqual([
      "mic-off",
      "mic-off",
      "volume-x",
      "ban",
    ]);
    expect(screen.getByRole("img")).toHaveAccessibleName(
      "5 people: Ana (muted), Ben (muted by server, muted for you), " +
        "Cara (deafened, muted for you), Dan (suppressed (cannot speak here)), Eve",
    );
  });

  describe("on a measured line", () => {
    let report: () => void = () => undefined;

    beforeEach(() => {
      vi.stubGlobal(
        "ResizeObserver",
        class {
          constructor(callback: ResizeObserverCallback) {
            report = () => {
              const line = document.querySelector(".avatar-stack__line");
              callback([{ target: line } as ResizeObserverEntry], this);
            };
          }
          observe() {}
          unobserve() {}
          disconnect() {}
        },
      );
    });
    afterEach(() => {
      vi.unstubAllGlobals();
    });

    /** Lays the line out `rem` wide, with an optional toggle beside the stack. */
    function layOut(container: HTMLElement, rem: number, toggleRem = 0) {
      const line = container.querySelector(".avatar-stack__line") as HTMLElement;
      Object.defineProperty(line, "clientWidth", { configurable: true, value: rem * 16 });
      const toggle = line.querySelector("button");
      if (toggle) {
        toggle.getBoundingClientRect = () => ({ width: toggleRem * 16 }) as DOMRect;
      }
      act(() => {
        report();
      });
    }

    it("shows as many avatars as fit beside the toggle and counts the rest in the chip", () => {
      const { container } = render(
        <div className="avatar-stack__line">
          <AvatarStack people={people(5)} total={9} />
          <button type="button">Show all</button>
        </div>,
      );
      layOut(container, 12, 7);
      expect(container.querySelectorAll(".avatar")).toHaveLength(3);
      expect(container.querySelector(".avatar-stack__more")).toHaveAttribute("data-more", "+6");
      expect(screen.getByRole("img")).toHaveAccessibleName(
        "3 of 9 people: Ana, Ben, Cara and 6 more",
      );
      layOut(container, 19, 7);
      expect(container.querySelectorAll(".avatar")).toHaveLength(5);
    });

    it("measures the tree once and each tree level once, not every line", () => {
      const observed: Element[] = [];
      vi.stubGlobal(
        "ResizeObserver",
        class {
          observe(target: Element) {
            observed.push(target);
          }
          unobserve() {}
          disconnect() {}
        },
      );
      // Widths come from data attributes: refs attach only after the stacks' layout effects ran.
      const reads: Record<string, number> = {};
      const original = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
      Object.defineProperty(HTMLElement.prototype, "clientWidth", {
        configurable: true,
        get(this: HTMLElement) {
          const { key, rem } = this.dataset;
          if (key) reads[key] = (reads[key] ?? 0) + 1;
          return Number(rem ?? 0) * 16;
        },
      });
      const line = (key: string, rem: number, total: number) => (
        <div className="avatar-stack__line" data-key={key} data-rem={rem}>
          <AvatarStack people={people(5)} total={total} />
        </div>
      );
      let container: HTMLElement;
      try {
        ({ container } = render(
          <div role="tree" data-rem={20}>
            <div aria-level={2}>{line("a", 12, 9)}</div>
            <div aria-level={2}>{line("b", 12, 9)}</div>
            <div aria-level={3}>{line("c", 5, 14)}</div>
          </div>,
        ));
      } finally {
        // jsdom defines clientWidth on Element.prototype; the override shadows it until removed.
        if (original) Object.defineProperty(HTMLElement.prototype, "clientWidth", original);
        else Reflect.deleteProperty(HTMLElement.prototype, "clientWidth");
      }
      expect(observed).toHaveLength(1);
      expect(observed[0]).toHaveAttribute("role", "tree");
      expect(reads).toEqual({ a: 1, c: 1 });
      const counts = Array.from(container.querySelectorAll(".avatar-stack")).map(
        (stack) => stack.querySelectorAll(".avatar").length,
      );
      expect(counts).toEqual([5, 5, 3]);
    });
  });
});

describe("ChannelTree avatar stack", () => {
  it("lists names in small expanded channels without a stack", () => {
    renderTree(serverState(4));
    expect(item(/^Lobby/).querySelector(".avatar-stack")).toBeNull();
    expect(item(/^Raid/).querySelector(".avatar-stack")).toBeNull();
    expect(item(/^Ana/)).toBeInTheDocument();
  });

  it("shows the stack instead of names when a channel is collapsed", () => {
    renderTree(serverState(4));
    fireEvent.click(item(/^Lobby/).querySelector(".tree-channel__toggle") as Element);
    const small = item(/^Lobby/);
    expect(small).toHaveAttribute("aria-expanded", "false");
    expect(within(small).getByRole("img")).toHaveAccessibleName("3 people: Xia, Yan, Zed");
    expect(screen.queryByRole("treeitem", { name: /^Xia/ })).toBeNull();
  });

  it("shows the stack and a Show all toggle for crowded expanded channels", async () => {
    const { container } = renderTree(serverState(14, [12]));
    const crowd = item(/^Raid/);
    expect(crowd).toHaveAttribute("aria-expanded", "true");
    expect(within(crowd).getByRole("img")).toHaveAccessibleName(
      "5 of 14 people: Lou (talking), Ana, Ben, Cara, Dan and 9 more",
    );
    expect(crowd.querySelector(".avatar[data-talking='true']")).not.toBeNull();
    expect(screen.queryByRole("treeitem", { name: /^Ana/ })).toBeNull();

    const toggle = screen.getByRole("button", { name: "Show all (14)" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(item(/^Ana/)).toBeInTheDocument();
    expect(item(/^Raid/).querySelector(".avatar-stack")).toBeNull();
    const fewer = screen.getByRole("button", { name: "Show fewer" });
    expect(fewer).toHaveAttribute("aria-expanded", "true");
    expect(await axeViolations(container)).toEqual([]);
  });

  it("keeps the toggle out of the row's Enter handling", () => {
    renderTree(serverState(14));
    const toggle = screen.getByRole("button", { name: "Show all (14)" });
    fireEvent.keyDown(toggle, { key: "Enter" });
    expect(screen.queryByRole("treeitem", { name: /^Ana/ })).toBeNull();
  });
});
