import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi, type User } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { AvatarStack } from "./AvatarStack";
import { ChannelTree } from "./ChannelTree";
import { getServerState, type ServerState } from "./serverStore";
import { resetAll } from "./testUtils";
import { stackPeople, stackSize } from "./tree";

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

it("shows fewer avatars on deeper tree levels", () => {
  expect([1, 2, 3, 4, 8].map(stackSize)).toEqual([4, 3, 2, 1, 1]);
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
      "3 of 14 people: Lou (talking), Ana, Ben and 11 more",
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
