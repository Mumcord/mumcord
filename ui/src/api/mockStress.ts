/**
 * Opt-in stress data for the mock engine (seed "stress"): a big community server with ~300
 * nested channels (304 plus root, up to 5 levels) and 1000 people, for scale measurements and screenshots in a
 * browser. Mirrors the load-test server layout in `testenv/scale/seed_channels.py`. Fictional
 * names only.
 */
import type { ChannelInfo, User } from "../bindings/commands";
import type { MockApi } from "./mock";

declare global {
  interface Window {
    /** The stress-seeded mock, for the scale measuring script (seed "stress" only). */
    __mumcordMock?: MockApi;
  }
}

const GAMES = [
  "Starfall",
  "Iron Tide",
  "Neon Drift",
  "Hollow Crown",
  "Skyforge",
  "Deep Rust",
  "Ember Line",
  "Frost Hollow",
  "Void Runner",
  "Tidebreak",
  "Cinder Peak",
  "Rogue Signal",
];
const CLANS = [
  "Night Owls",
  "Red Foxes",
  "Blue Herons",
  "Grey Wolves",
  "Gold Lions",
  "Black Cats",
  "White Ravens",
  "Green Vipers",
  "Silver Hawks",
  "Iron Bears",
  "Storm Crows",
  "Sand Eagles",
  "Moon Moths",
  "Sun Bees",
  "Fire Ants",
];
const TOPICS = [
  "Tech Talk",
  "Movies & TV",
  "Books",
  "Art & Design",
  "Cooking",
  "Sports",
  "Travel",
  "Pets",
];
const FIRST = [
  "Alex", "Mia", "Noah", "Emma", "Liam", "Olivia", "Elias", "Sofia", "Finn", "Lena", "Jonas",
  "Hanna", "Ben", "Clara", "Paul", "Lea", "Leon", "Marie", "Luca", "Anna", "Felix", "Ella",
  "Max", "Ida", "Theo", "Nora", "Emil", "Frieda", "Oskar", "Greta", "Samir", "Aylin", "Kenji",
  "Yuki", "Priya", "Ravi", "Diego", "Lucia", "Omar", "Zara",
]; // prettier-ignore
const SUFFIX = [
  "", "_gg", "92", "TheGreat", "_x", "77", "Wolf", "_live", "Pixel", "07", "Nova", "_afk",
  "Storm", "2k", "Byte", "_88", "Fox", "Ace", "_pro", "Sky", "Moon", "Rex", "_tv", "Zen", "Dash",
]; // prettier-ignore

const allow = { enter: true, textMessage: true, listen: true, registerSelf: false };

/** Channel the own user lands in: "Starfall › Squads › Squad 2 › Team Blue" (level 5). */
export const STRESS_SELF_CHANNEL_PATH = ["Gaming", "Starfall", "Squads", "Squad 2", "Team Blue"];

export function stressChannels(rootName: string): ChannelInfo[] {
  const channels: ChannelInfo[] = [
    {
      id: 0,
      parentId: null,
      name: rootName,
      position: 0,
      permissions: { ...allow },
      description: null,
      hasDescription: false,
    },
  ];
  const add = (parentId: number, name: string): number => {
    const id = channels.length;
    channels.push({
      id,
      parentId,
      name,
      position: 0,
      permissions: { ...allow },
      description: null,
      hasDescription: false,
    });
    return id;
  };
  add(0, "Welcome Lobby");
  add(0, "AFK");
  const stage = add(0, "Event Stage");
  add(stage, "Backstage");
  add(stage, "Q&A Queue");
  const gaming = add(0, "Gaming");
  for (const game of GAMES) {
    const g = add(gaming, game);
    add(g, "Casual");
    add(g, "Ranked");
    const squads = add(g, "Squads");
    for (let n = 1; n <= 4; n++) {
      const squad = add(squads, `Squad ${String(n)}`);
      if (n <= 2) {
        add(squad, "Team Red");
        add(squad, "Team Blue");
      }
    }
  }
  const clans = add(0, "Clans");
  for (const clan of CLANS) {
    const c = add(clans, clan);
    add(c, "Lounge");
    const ops = add(c, "Ops");
    for (const unit of ["Alpha", "Bravo", "Charlie"]) add(ops, unit);
  }
  const community = add(0, "Community");
  for (const topic of TOPICS) {
    const t = add(community, topic);
    for (const room of ["Chat", "Voice 1", "Voice 2"]) add(t, room);
  }
  const music = add(0, "Music");
  for (const room of ["Listening Party", "Jam Room 1", "Jam Room 2", "Jam Room 3", "Jam Room 4"]) {
    add(music, room);
  }
  add(music, "Karaoke");
  const study = add(0, "Study Hall");
  for (let n = 1; n <= 10; n++) add(study, `Silent ${String(n)}`);
  add(study, "Pomodoro");
  for (let n = 1; n <= 5; n++) add(study, `Group Study ${String(n)}`);
  const support = add(0, "Support");
  add(support, "Help Desk");
  add(support, "Waiting Room");
  const staff = add(support, "Staff");
  add(staff, "Moderators");
  add(staff, "Admins");
  return channels;
}

/** Deterministic PRNG (xorshift32) so every run shows the same crowd. */
function rng(seed: number): () => number {
  let x = seed;
  return () => {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    return (x >>> 0) / 0x1_0000_0000;
  };
}

export function stressName(n: number): string {
  const first = FIRST[n % FIRST.length] ?? "User";
  const suffix = SUFFIX[Math.floor(n / FIRST.length) % SUFFIX.length] ?? "";
  const round = Math.floor(n / (FIRST.length * SUFFIX.length));
  return round === 0 ? `${first}${suffix}` : `${first}${suffix}${String(round)}`;
}

/**
 * Channel for each of `count` people: three crowded rooms (Welcome Lobby, AFK, Event Stage),
 * one busy game room, a long tail over ~110 channels, the rest empty.
 */
export function stressPlan(channels: readonly ChannelInfo[], count: number): number[] {
  const random = rng(0x9e3779b9);
  const byName = (name: string) => channels.find((c) => c.name === name)?.id ?? 1;
  const casual = channels.find((c) => c.name === "Casual")?.id ?? 1;
  const plan: number[] = [];
  const fixed: [number, number][] = [
    [byName("Welcome Lobby"), 0.14],
    [byName("AFK"), 0.11],
    [byName("Event Stage"), 0.12],
    [casual, 0.06],
  ];
  for (const [id, share] of fixed) {
    for (let i = 0; i < Math.round(count * share); i++) plan.push(id);
  }
  const used = new Set(fixed.map(([id]) => id));
  const tail = channels.filter((c) => c.parentId !== null && !used.has(c.id)).map((c) => c.id);
  for (let i = tail.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [tail[i], tail[j]] = [tail[j] ?? 0, tail[i] ?? 0];
  }
  tail.length = Math.min(tail.length, 110);
  const weights = tail.map((_, i) => 1 / Math.pow(i + 1, 0.9));
  const total = weights.reduce((a, b) => a + b, 0);
  while (plan.length < count) {
    let pick = random() * total;
    let chosen = tail[0] ?? 1;
    for (let i = 0; i < tail.length; i++) {
      const w = weights[i] ?? 0;
      if (pick < w) {
        chosen = tail[i] ?? chosen;
        break;
      }
      pick -= w;
    }
    plan.push(chosen);
  }
  return plan;
}

/** 999 other people (plus `self` = 1000), about a third with an avatar, some muted or deafened. */
export function stressUsers(self: User, channels: readonly ChannelInfo[]): User[] {
  const random = rng(0x12345678);
  const plan = stressPlan(channels, 999);
  const others = plan.map((channelId, i): User => {
    const session = i + 2;
    const hasAvatar = random() < 0.33;
    const roll = random();
    return {
      session,
      channelId,
      name: stressName(i),
      certHash: `stress${String(session)}`,
      registered: random() < 0.5,
      selfMute: roll < 0.15,
      selfDeaf: roll < 0.05,
      serverMute: false,
      serverDeaf: false,
      suppressed: false,
      listeningChannelIds: [],
      hasAvatar,
      avatarHash: hasAvatar ? `stress-${String(session)}` : null,
      hasComment: random() < 0.2,
      volume: 100,
      localMute: false,
    };
  });
  return [self, ...others];
}
