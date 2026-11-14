import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import type { MediaPacket } from "../../api";
import { tokenizeJson } from "./json";
import {
  exportJson,
  exportMarkdown,
  filterPackets,
  formatGap,
  formatHex,
  formatSize,
  formatTime,
  kindKey,
  MAX_PACKETS,
  mediaFacts,
  mediaText,
  mergePackets,
  packetBlock,
  packetKinds,
  type Names,
  type Packet,
} from "./packets";

const at = new Date(2026, 9, 28, 18, 42, 3, 120).getTime();

function packet(over: Partial<Packet> = {}): Packet {
  return {
    id: 1,
    serverId: 1,
    atMs: at,
    direction: "sent",
    kind: "mumcord.watch",
    pluginData: true,
    size: 412,
    sender: null,
    receivers: [12],
    delayMs: 340,
    tokensLeft: 2,
    decoded: '{\n  "v": 1\n}',
    rawHex: "7b7d",
    media: null,
    ...over,
  };
}

const names: Names = {
  user: (_server, session) => (session === 12 ? "Alice" : null),
  server: () => "Lobby server",
};
const info = { appVersion: "0.4.3", os: "Linux x86_64", exportedAtMs: at + 5000 };

describe("packet formats", () => {
  it("shows times with milliseconds, sizes in B or KB and gaps", () => {
    expect(formatTime(at)).toBe("18:42:03.120");
    expect(formatSize(412)).toBe("412 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatGap(340)).toBe("+340 ms");
    expect(formatGap(3200)).toBe("+3.2 s");
  });

  it("prints hex in rows of 16 bytes with offsets", () => {
    const hex = "00".repeat(17);
    expect(formatHex(hex).split("\n")).toEqual([
      `0000  ${Array(16).fill("00").join(" ")}`,
      "0010  00",
    ]);
  });
});

describe("packet list", () => {
  it("keeps at most the newest MAX_PACKETS packets", () => {
    const many = Array.from({ length: MAX_PACKETS }, (_, id) => packet({ id }));
    const merged = mergePackets(many, [packet({ id: MAX_PACKETS })]);
    expect(merged).toHaveLength(MAX_PACKETS);
    expect(merged[0]?.id).toBe(1);
    expect(merged.at(-1)?.id).toBe(MAX_PACKETS);
  });

  it("filters by server, direction and type", () => {
    const list = [
      packet({ id: 1 }),
      packet({ id: 2, direction: "received", sender: 7, receivers: [] }),
      packet({ id: 3, serverId: 2, kind: "UserState", pluginData: false }),
    ];
    const ids = (f: Parameters<typeof filterPackets>[1]) => filterPackets(list, f).map((p) => p.id);
    expect(ids({ serverId: null, direction: null, kind: null })).toEqual([1, 2, 3]);
    expect(ids({ serverId: 2, direction: null, kind: null })).toEqual([3]);
    expect(ids({ serverId: null, direction: "received", kind: null })).toEqual([2]);
    expect(ids({ serverId: null, direction: null, kind: "UserState" })).toEqual([3]);
    expect(packetKinds(list)).toEqual(["mumcord.watch", "UserState"]);
  });
});

describe("exports", () => {
  it("JSON is complete and names people", () => {
    const parsed = JSON.parse(exportJson([packet()], info, names)) as {
      warning: string;
      appVersion: string;
      os: string;
      timeRange: { from: string };
      packets: { localTime: string; to: { name: string }[]; decoded: unknown; pacer: unknown }[];
    };
    expect(parsed.warning).toMatch(/personal data/u);
    expect(parsed.appVersion).toBe("0.4.3");
    expect(parsed.os).toBe("Linux x86_64");
    expect(parsed.timeRange.from).toBe(new Date(at).toISOString());
    expect(parsed.packets[0]?.localTime).toBe("18:42:03.120");
    expect(parsed.packets[0]?.to[0]?.name).toBe("Alice");
    expect(parsed.packets[0]?.decoded).toEqual({ v: 1 });
    expect(parsed.packets[0]?.pacer).toEqual({ delayMs: 340, tokensLeft: 2 });
  });

  it("Markdown has a warning, a table row per packet and the decoded content", () => {
    const md = exportMarkdown(
      [packet(), packet({ id: 2, direction: "received", sender: 7, receivers: [], decoded: null })],
      info,
      names,
    );
    expect(md).toContain("May contain personal data");
    expect(md).toContain("- App version: 0.4.3");
    expect(md).toContain(
      "| 18:42:03.120 | ↑ sent | mumcord.watch | 412 B | → Alice (session 12) | delayed 340 ms, 2 tokens left |",
    );
    expect(md).toContain("| ↓ received | mumcord.watch | 412 B | ← session 7 |");
    expect(md).toContain('```json\n{\n  "v": 1\n}\n```');
  });

  it("Markdown keeps backslashes and pipes inside their table cell", () => {
    const tricky: Names = {
      user: (_server, session) => (session === 12 ? "Bob\\" : "a\\|b"),
      server: () => "Lobby server",
    };
    const md = exportMarkdown(
      [packet({ kind: "odd\\" }), packet({ id: 2, receivers: [13] })],
      info,
      tricky,
    );
    // A backslash from the value must not swallow or expose a column separator.
    expect(md).toContain("| odd\\\\ | 412 B | → Bob\\\\ (session 12) |");
    expect(md).toContain("| → a\\\\\\|b (session 13) |");
  });

  it("a copied packet has the gap, and fences survive backticks in the payload", () => {
    const block = packetBlock(packet({ decoded: '"```"' }), names, at - 340).join("\n");
    expect(block).toContain("- gap +340 ms");
    expect(block).toContain('````json\n"```"\n````');
  });
});

describe("media packets", () => {
  /** Translation stand-in: channel and frame keys come back as readable English. */
  const t = ((key: string, options?: Record<string, unknown>) =>
    key === "inspector.media.channel"
      ? `channel ${String(options?.channel)}`
      : key.replace("inspector.media.", "")) as unknown as TFunction<"settings">;

  const media = (over: Partial<MediaPacket> = {}): MediaPacket => ({
    format: "media",
    version: null,
    channel: 2,
    control: null,
    flags: 1,
    keyframe: true,
    ...over,
  });
  const mediaPacket = (over: Partial<MediaPacket> = {}) =>
    packet({
      kind: ".media",
      pluginData: false,
      decoded: null,
      rawHex: "",
      media: media(over),
    });

  it("lists a media message under its own type", () => {
    expect(kindKey(mediaPacket())).toBe("media");
    expect(kindKey(packet())).toBe("watch");
  });

  it("tells control messages, frames and the old header apart", () => {
    expect(mediaFacts(t, null)).toBeNull();
    expect(mediaFacts(t, media({ channel: 0, control: 2, flags: null, keyframe: false }))).toBe(
      "key",
    );
    expect(mediaFacts(t, media({ channel: 0, control: 3, flags: null }))).toBe("kf");
    expect(mediaFacts(t, media({ channel: 0, control: 42, flags: null }))).toBe("kind 42");
    expect(mediaFacts(t, media())).toBe("channel 2 · keyframe · 0x01");
    expect(mediaFacts(t, media({ channel: 3, flags: 0, keyframe: false }))).toBe(
      "channel 3 · delta · 0x00",
    );
    // The old header has neither a channel nor a flags byte; its keyframe bit still shows.
    expect(mediaFacts(t, media({ format: "legacy", version: 2, channel: null, flags: null }))).toBe(
      "keyframe",
    );
  });

  it("writes the media facts into both exports", () => {
    expect(mediaText(mediaPacket())).toBe(".media document, channel 2, keyframe, flags 0x01");
    expect(mediaText(packet())).toBe("");
    expect(mediaText(mediaPacket({ channel: 0, control: 0, flags: null, keyframe: false }))).toBe(
      ".media document, description",
    );
    expect(
      mediaText(
        mediaPacket({ format: "legacy", version: 1, channel: null, flags: null, keyframe: false }),
      ),
    ).toBe(".media legacy v1, delta frame");
    expect(packetBlock(mediaPacket(), names, null)).toContain(
      "- .media document, channel 2, keyframe, flags 0x01",
    );
    const { packets } = JSON.parse(exportJson([mediaPacket()], info, names)) as {
      packets: { media: MediaPacket | null }[];
    };
    expect(packets[0]?.media).toEqual(media());
  });
});

describe("JSON highlighting", () => {
  it("tells keys, strings, numbers and literals apart", () => {
    const kinds = tokenizeJson('{"a": "b", "n": -1.5, "t": true}')
      .filter((t) => t.kind !== "space" && t.kind !== "punctuation")
      .map((t) => `${t.kind}:${t.text}`);
    expect(kinds).toEqual([
      'key:"a"',
      'string:"b"',
      'key:"n"',
      "number:-1.5",
      'key:"t"',
      "literal:true",
    ]);
  });

  it("keeps markup as plain text", () => {
    const text = tokenizeJson('"<img src=x onerror=alert(1)>"')
      .map((t) => t.text)
      .join("");
    expect(text).toBe('"<img src=x onerror=alert(1)>"');
  });
});
