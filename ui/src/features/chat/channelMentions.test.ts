import { describe, expect, it } from "vitest";
import {
  findChannelQuery,
  insertChannelMention,
  linkChannelMentions,
  rankChannelCandidates,
  type ChannelChoice,
} from "./channelMentions";

const channels: ChannelChoice[] = [
  { id: 1, name: "Lobby", path: "Lobby" },
  { id: 2, name: "Raid", path: "Raid" },
  { id: 3, name: "Raid night", path: "Gaming › Raid night" },
  { id: 4, name: "AFK", path: "Lobby › AFK" },
  { id: 5, name: "AFK", path: "Gaming › AFK" },
  { id: 6, name: "Music Room", path: "Music Room" },
];
const label = (name: string) => `#${name}, channel`;

function chips(html: string) {
  const doc = new DOMParser().parseFromString(
    linkChannelMentions(html, channels, label),
    "text/html",
  );
  return [...doc.querySelectorAll(".chat-channel-mention")].map((c) => ({
    text: c.textContent,
    ids: (c as HTMLElement).dataset["channelIds"],
  }));
}

describe("channel mentions (#643)", () => {
  it("finds the # query at the caret, also across a space", () => {
    expect(findChannelQuery("see #Ra", 7)).toEqual({ start: 4, query: "Ra" });
    expect(findChannelQuery("#Raid n", 7)).toEqual({ start: 0, query: "Raid n" });
    expect(findChannelQuery("a#b", 3)).toBeNull();
    expect(findChannelQuery("# x", 3)).toBeNull();
  });

  it("ranks prefix matches before inner ones and inserts the full name", () => {
    expect(rankChannelCandidates(channels, "ra").map((c) => c.id)).toEqual([2, 3]);
    expect(rankChannelCandidates(channels, "room").map((c) => c.id)).toEqual([6]);
    expect(insertChannelMention("go #ra later", 3, 6, "Raid night")).toEqual({
      text: "go #Raid night later",
      caret: 15,
    });
  });

  it("links the longest existing name, stops at punctuation and leaves unknown names alone", () => {
    expect(chips("<p>#Raid night tonight? Or #Raid.</p>")).toEqual([
      { text: "#Raid night", ids: "3" },
      { text: "#Raid", ids: "2" },
    ]);
    expect(chips("<p>#Music Room! and #Nope and #Raidnight</p>")).toEqual([
      { text: "#Music Room", ids: "6" },
    ]);
    expect(chips("<p>mail a#Lobby</p>")).toEqual([]);
  });

  it("gives channels sharing a name one chip that lists them all", () => {
    expect(chips("<p>back in #afk</p>")).toEqual([{ text: "#afk", ids: "4,5" }]);
  });

  it("leaves text inside links and code as it is", () => {
    expect(chips('<p><a href="https://x">#Lobby</a> <code>#Lobby</code> #Lobby</p>')).toEqual([
      { text: "#Lobby", ids: "1" },
    ]);
  });
});
