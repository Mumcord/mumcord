import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi, User } from "../../api";
import { axeViolations } from "../../test/axe";
import { applyThemeToDocument } from "../../theme/theme";
import { currentPersonMenu } from "../members/personMenuStore";
import { ProfilePreviewHost } from "../members/ProfilePreview";
import { ChannelsPanel } from "./ChannelsPanel";
import { orderedModules } from "./mumcord";
import { getServerState } from "./serverStore";
import { connectedMock, resetAll, stubDialog } from "./testUtils";

let api: MockApi;

/** Someone in Lobby, built from Anna so every field is filled in. */
function someone(session: number, name: string, extra: Partial<User>): User {
  const anna = getServerState().users.get(2);
  if (!anna) throw new Error("Anna is missing");
  return { ...anna, session, name, certHash: `${String(session)}hash`, ...extra };
}

const RELAY = someone.bind(null, 20, "Night Owls Relay");
const DJ = someone.bind(null, 21, "DJ Byte");
const TESTER = someone.bind(null, 22, "Testy");

function addBots() {
  act(() => {
    api.mock.upsertUser(
      RELAY({
        isMumcord: true,
        product: "Mumbot",
        version: "0.4.1",
        bot: { voice: false, modules: ["history", "peer", "webhook"] },
      }),
    );
    api.mock.upsertUser(
      DJ({
        isMumcord: true,
        product: "Mumbot",
        version: "0.4.1",
        bot: { voice: true, modules: ["music"] },
      }),
    );
    // The tester bot sends no bot marker and must look like a person.
    api.mock.upsertUser(TESTER({ isMumcord: true, bot: null }));
  });
}

async function renderPanel({ expanded = true } = {}) {
  api = await connectedMock({ bindStore: true });
  const view = render(
    <>
      <ChannelsPanel />
      <ProfilePreviewHost />
    </>,
  );
  await screen.findByRole("tree", { name: "Channels" });
  addBots();
  if (expanded) expand();
  return view;
}

const section = () => screen.getByRole("region", { name: /Bots on this server/ });
const toggle = () => screen.getByRole("button", { name: /^Bots on this server \(\d+\)$/ });

/** The section starts collapsed; the open state is remembered, so expand only when needed. */
function expand() {
  if (toggle().getAttribute("aria-expanded") === "false") fireEvent.click(toggle());
}
const botButton = (name: RegExp) => within(section()).getByRole("button", { name });

beforeEach(stubDialog);
afterEach(resetAll);

describe("BotsSection", () => {
  it("starts collapsed, with the number of bots in the heading", async () => {
    localStorage.removeItem("mumcord.section.open.server-bots");
    await renderPanel({ expanded: false });
    expect(screen.getByRole("button", { name: "Bots on this server (2)" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(within(section()).queryByRole("button", { name: /DJ Byte/ })).toBeNull();
  });

  it("lists every bot with what it does, the tester not", async () => {
    await renderPanel();
    expect(
      botButton(/^Night Owls Relay Bot, in Lobby · Relay · Chat history · Webhooks$/),
    ).toBeVisible();
    expect(botButton(/^DJ Byte Bot, in Lobby · Plays music$/)).toBeVisible();
    expect(within(section()).queryByRole("button", { name: /Testy/ })).toBeNull();
  });

  it("shows the current channel of a hidden bot and follows it when it moves", async () => {
    await renderPanel();
    const other = [...getServerState().channels.values()].find((c) => c.id !== 0 && c.id !== 1);
    if (!other) throw new Error("no second channel");
    act(() => {
      api.mock.upsertUser(
        RELAY({ channelId: other.id, isMumcord: true, bot: { voice: false, modules: [] } }),
      );
    });
    expect(botButton(/^Night Owls Relay Bot, in /)).toHaveTextContent(`in ${other.name}`);
    expect(
      within(screen.getByRole("tree", { name: "Channels" })).queryByText("Night Owls Relay"),
    ).toBeNull();
  });

  it("keeps the relay out of the tree, shows the voice bot there with the badge", async () => {
    await renderPanel();
    const tree = screen.getByRole("tree", { name: "Channels" });
    expect(within(tree).queryByText("Night Owls Relay")).toBeNull();
    expect(within(tree).getByRole("treeitem", { name: /^DJ Byte Bot/ })).toBeVisible();
    const tester = within(tree).getByRole("treeitem", { name: /^Testy Uses Mumcord/ });
    expect(within(tester).queryByRole("img", { name: "Bot" })).toBeNull();
  });

  it("hides when no bot is on the server", async () => {
    api = await connectedMock({ bindStore: true });
    render(<ChannelsPanel />);
    await screen.findByRole("tree", { name: "Channels" });
    expect(screen.queryByRole("region", { name: /Bots on this server/ })).toBeNull();
  });

  it("collapses and expands like the member sections", async () => {
    await renderPanel();
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "false");
    expect(within(section()).queryByRole("button", { name: /DJ Byte/ })).toBeNull();
    fireEvent.click(toggle());
    expect(toggle()).toHaveAttribute("aria-expanded", "true");
  });

  it("moves focus with the arrow keys behind one Tab stop", async () => {
    await renderPanel();
    // Bots listed nowhere else come first, as in the mockups.
    const relay = botButton(/^Night Owls Relay/);
    const dj = botButton(/^DJ Byte/);
    expect(relay).toHaveAttribute("tabindex", "0");
    expect(dj).toHaveAttribute("tabindex", "-1");
    relay.focus();
    fireEvent.keyDown(relay, { key: "ArrowDown" });
    expect(dj).toHaveFocus();
    expect(dj).toHaveAttribute("tabindex", "0");
    fireEvent.keyDown(dj, { key: "Home" });
    expect(relay).toHaveFocus();
    fireEvent.keyDown(relay, { key: "End" });
    expect(dj).toHaveFocus();
  });

  it("opens the person menu for a bot, as for people", async () => {
    await renderPanel();
    fireEvent.contextMenu(botButton(/^Night Owls Relay/));
    expect(currentPersonMenu()?.session).toBe(20);
  });

  it("orders known modules by importance, unknown ones last", () => {
    expect(orderedModules(["webhook", "history", "peer"])).toEqual(["peer", "history", "webhook"]);
    expect(orderedModules(["zeta", "music", "alpha"])).toEqual(["music", "alpha", "zeta"]);
  });

  it.each(["dark", "light"] as const)("has no axe violations (%s)", async (theme) => {
    applyThemeToDocument(theme);
    const { container } = await renderPanel();
    expect(await axeViolations(container)).toEqual([]);
  });
});
