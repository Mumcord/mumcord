import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { MockApi, User } from "../../api";
import { applyLanguage } from "../../i18n";
import { MembersPanel } from "../members/MembersPanel";
import { ChannelsPanel } from "./ChannelsPanel";
import { getServerState } from "./serverStore";
import { voiceState } from "./statusText";
import { connectedMock, resetAll, stubDialog } from "./testUtils";

const ANNA = 2;

let api: MockApi;

beforeEach(stubDialog);
afterEach(resetAll);

function base(): User {
  const user = getServerState().users.get(ANNA);
  if (!user) throw new Error("Anna missing from the seeded server");
  return user;
}

function update(change: Partial<User>) {
  act(() => {
    api.mock.emit("userUpserted", { user: { ...base(), ...change } });
  });
}

async function renderBoth() {
  api = await connectedMock();
  render(
    <>
      <ChannelsPanel />
      <MembersPanel />
    </>,
  );
  await screen.findByRole("tree", { name: "Channels" });
  await screen.findByRole("heading", { name: "In Lobby (3)" });
}

const treeRow = () => screen.getByRole("treeitem", { name: /^Anna/ });
const memberRow = () => screen.getByRole("button", { name: /^Anna/ });
const rows = () => [treeRow(), memberRow()];
const icons = (row: HTMLElement) =>
  [...row.querySelectorAll(".person-icon:not(.person-icon--registered)")].map((icon) =>
    icon.getAttribute("class"),
  );

describe("voiceState", () => {
  const user = (change: Partial<User>) => ({ ...plain, ...change });
  const plain: User = {
    session: 1,
    channelId: 0,
    name: "P",
    certHash: null,
    registered: true,
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
    isMumcord: false,
    mumcordFeatures: [],
  };

  it("puts what the server imposes before the person's own choice", () => {
    expect(voiceState(plain)).toBeNull();
    expect(voiceState(user({ selfMute: true }))).toBe("muted");
    expect(voiceState(user({ selfMute: true, suppressed: true }))).toBe("suppressed");
    expect(voiceState(user({ suppressed: true, serverMute: true }))).toBe("serverMuted");
    expect(voiceState(user({ serverMute: true, selfDeaf: true }))).toBe("deafened");
    expect(voiceState(user({ selfDeaf: true, serverDeaf: true }))).toBe("serverDeafened");
  });
});

describe("person status in the tree and member list", () => {
  it("marks a server mute with a distinct icon and spoken text", async () => {
    await renderBoth();
    update({ serverMute: true });
    for (const row of rows()) {
      expect(row).toHaveAccessibleName(/muted by server/);
      expect(icons(row)).toEqual(["person-icon person-icon--server"]);
      expect(row.querySelector("[data-icon='mic-off']")).not.toBeNull();
    }
  });

  it("marks a server deafen", async () => {
    await renderBoth();
    update({ serverDeaf: true, serverMute: true });
    for (const row of rows()) {
      expect(row).toHaveAccessibleName(/deafened by server/);
      expect(row).not.toHaveAccessibleName(/muted/);
      expect(row.querySelector(".person-icon--server[data-icon='headphones-off']")).not.toBeNull();
    }
  });

  it("marks a suppressed person", async () => {
    await renderBoth();
    update({ suppressed: true });
    for (const row of rows()) {
      expect(row).toHaveAccessibleName(/suppressed \(cannot speak here\)/);
      expect(icons(row)).toEqual(["person-icon person-icon--suppressed"]);
    }
  });

  it("keeps the self mute look apart from the server one", async () => {
    await renderBoth();
    update({ selfMute: true });
    for (const row of rows()) {
      expect(row).toHaveAccessibleName(/, muted$/);
      expect(icons(row)).toEqual(["person-icon"]);
    }
  });

  it("shows the Mumcord badge in tree and member list only for Mumcord users", async () => {
    await renderBoth();
    for (const row of rows()) expect(row.querySelector(".mumcord-badge")).toBeNull();
    update({ isMumcord: true, mumcordFeatures: ["now-playing"] });
    for (const row of rows()) {
      expect(row.querySelector(".mumcord-badge")).toHaveAttribute("title", "Uses Mumcord");
    }
  });

  it("shows a person muted for you in red with the local-mute icon", async () => {
    await renderBoth();
    const names = () =>
      [treeRow(), memberRow()].map((row) => row.querySelector("[data-local-muted]"));
    for (const name of names()) expect(name).toHaveAttribute("data-local-muted", "false");
    update({ localMute: true, selfMute: true });
    for (const name of names()) expect(name).toHaveAttribute("data-local-muted", "true");
    for (const row of rows()) {
      expect(row).toHaveAccessibleName(/muted, muted for you/);
      expect(row.querySelector(".person-icon--local-muted[data-icon='volume-x']")).not.toBeNull();
      expect(row.querySelector("[data-icon='mic-off']")).not.toBeNull();
    }
  });

  it("speaks the server states in Spanish and German", async () => {
    await renderBoth();
    update({ serverMute: true });
    await act(() => applyLanguage("es"));
    expect(treeRow()).toHaveAccessibleName(/silenciado por el servidor/);
    await act(() => applyLanguage("de"));
    expect(treeRow()).toHaveAccessibleName(/vom Server stummgeschaltet/);
    await act(() => applyLanguage("en"));
  });
});

describe("status icon tooltips", () => {
  const cases: [string, Partial<User>, string][] = [
    ["registered", { registered: true }, "Registered on this server"],
    ["guest", { registered: false }, "Not registered on this server (guest)"],
    ["self-muted", { selfMute: true }, "Muted"],
    ["self-deafened", { selfDeaf: true }, "Deafened"],
    ["server-muted", { serverMute: true }, "Muted by the server"],
    ["server-deafened", { serverDeaf: true }, "Deafened by the server"],
    ["suppressed", { suppressed: true }, "Suppressed – can't speak in this channel"],
    ["locally muted", { localMute: true }, "Muted for you"],
  ];
  it.each(cases)("%s icon has a tooltip", async (_n, change, text) => {
    await renderBoth();
    update({ registered: true, ...change });
    expect((await screen.findAllByTitle(text)).length).toBeGreaterThan(0);
  });

  it("shows the guest icon only for unregistered people", async () => {
    await renderBoth();
    update({ registered: true });
    expect(treeRow().querySelector('[data-icon="user-guest"]')).toBeNull();
    expect(treeRow().querySelector('[data-icon="user-check"]')).not.toBeNull();
    update({ registered: false });
    expect(treeRow().querySelector('[data-icon="user-guest"]')).not.toBeNull();
  });
});
