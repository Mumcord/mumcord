// Spec 08, the `mumcord.camera` wire: AC-8.25 (the messages are Bovnar documents with the fields
// of §6.5), AC-8.26 (hand-made, broken and oversized documents are ignored) and AC-8.27 (a
// client without camera support sees no camera and no error; chat keeps working).
//
// AC-8.26 sends the broken documents with a minimal Mumble client written for this spec
// (`RawClient` in flows.ts), because mumbot only sends well-formed messages.
//
// AC-8.27 runs with mumbot as the client without camera support (it ignores unknown plugin data
// like Mumcord 0.6.2 and the official Mumble client do). Manual part, with a real Mumcord 0.6.2
// build in the channel (the E2E image carries only the current build):
//   - 0.6.2 shows no camera icon, no tile, no error and no chat line while A's camera is on;
//   - chat, history sync and polls between 0.6.2, A and B keep working.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/signal.e2e.ts
import { $, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { botInBackground } from "../../helpers/bot.ts";
import { sendText, waitMessage } from "../../helpers/chat-ui.ts";
import {
  NAMES,
  RawClient,
  cameraMessages,
  cameraRoom,
  cameraView,
  clientLog,
  on,
  ownSession,
  sessionOf,
  setRoomOpen,
  sleep,
  startCamClient,
  startCapture,
  turnCameraOn,
  waitSameChannel,
  waitTile,
} from "./flows.ts";

const IGNORED = "ignoring a camera message";
const ignoredLines = (log: string): string[] => log.split("\n").filter((l) => l.includes(IGNORED));
const MARK = "fuzzmark";

(relayenv.relayAvailable() ? describe : describe.skip)("camera signalling (AC-8.25 to 8.27)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;

  before(async () => {
    ({ port } = await relayenv.startRelay("all"));
    await startCamClient("a", port);
    await startCamClient("b", port, { debugLog: true });
  });

  after(async () => {
    await apps.stopB();
    await relayenv.stopRelay();
  });

  it("AC-8.25: `on` and `cam-join` are Bovnar documents with the fields of §6.5", async () => {
    await startCapture();
    await on("b", async () => {
      await startCapture();
      await setRoomOpen(true);
    });
    const view = await turnCameraOn();
    expect(view.broadcast).toMatch(/^[0-9a-f]{16}$/);
    const alice = await ownSession();

    await on("b", async () => {
      await waitTile(NAMES.a);
      const onMsg = (await cameraMessages()).find(
        (m) => m.direction === "received" && m.sender === alice && m.fields?.t === "on",
      );
      expect(onMsg).toBeDefined();
      // §6.5: `mv` is 2 on the new media profile and absent on the old one. The inspector
      // writes a missing `mv` as null. The pinned relay is old, so this run sees null.
      const fields = onMsg?.fields ?? {};
      const { mv, ...rest } = fields;
      expect(rest).toEqual({ t: "on", id: view.broadcast });
      expect(mv === undefined || mv === null || mv === 2).toBe(true);
      // Bovnar text, never JSON (§6.5): `.v=<uint:8>1;.t="on";.id="…";`; the encoder writes the
      // integer's Bovnar type, a hand-written document may leave it out.
      expect(onMsg?.raw.trimStart().startsWith("{")).toBe(false);
      expect(onMsg?.raw).toMatch(/\.v\s*=\s*(<uint:\d+>)?\s*1\b/);
      expect(onMsg?.raw).toMatch(/\.t\s*=\s*"on"/);
    });

    const bob = await sessionOf(NAMES.b);
    const join = (await cameraMessages()).find(
      (m) => m.direction === "received" && m.sender === bob && m.fields?.t === "cam-join",
    );
    expect(join).toBeDefined();
    expect(join?.fields?.id).toBe(view.broadcast);
    expect(String(join?.fields?.codecs)).toMatch(/av1|vp9/);
    expect(join?.raw.trimStart().startsWith("{")).toBe(false);
    expect(join?.raw).toMatch(/\.codecs\s*=\s*"[a-z0-9,]+"/);
  });

  it("AC-8.26: broken, JSON and oversized documents are ignored without a trace of their content", async () => {
    const seen = ignoredLines(clientLog("b")).length;
    const roomBefore = await on("b", cameraRoom);
    const id = "0123456789abcdef";
    const docs = [
      // Truncated in the middle of a string.
      `.v = 1; .t = "on"; .id = "${MARK}`,
      // `t` must be a string.
      `.v = 1; .t = 5; .id = "${id}"; .x = "${MARK}";`,
      // The same message as JSON.
      JSON.stringify({ v: 1, t: "on", id, x: MARK }),
      // Over the 512-byte limit.
      `.v = 1; .t = "on"; .id = "${id}"; .x = "${MARK}${"a".repeat(600)}";`,
    ];

    const mallory = new RawClient("Mallory");
    await mallory.connect(port);
    try {
      await mallory.joinChannelOf(NAMES.b);
      // Her own echo is not Bob's view. Until he has applied the move he refuses every
      // document as not-same-channel, and that check runs before too-large or wrong-type.
      // She has to stay connected while he reads them: closing first lets the server drop
      // her before it forwards the documents, and he then ignores them for that reason.
      await on("b", () => waitSameChannel("Mallory"));
      const target = mallory.sessionOf(NAMES.b);
      const sendDocs = (): void => {
        for (const doc of docs) mallory.pluginData(target, "mumcord.camera", Buffer.from(doc, "utf8"));
        mallory.ping();
      };
      sendDocs();
      // The log file is written out every 5 s, so the ignore lines are not on disk at once.
      let fresh = "";
      let resent = false;
      const deadline = Date.now() + 12_000;
      while (Date.now() < deadline) {
        const lines = ignoredLines(clientLog("b"));
        fresh = lines.slice(seen).join("\n");
        if (/too-large/.test(fresh) && /wrong-type|malformed/.test(fresh)) break;
        if (!resent && lines.length - seen >= docs.length) {
          resent = true;
          await on("b", () => waitSameChannel("Mallory"));
          sendDocs();
        }
        await sleep(500);
      }
      expect(fresh).toMatch(/too-large/);
      expect(fresh).toMatch(/wrong-type|malformed/);
    } finally {
      await mallory.close();
    }

    await on("b", async () => {
      // Still responsive and unchanged: A's tile plays, no tile for Mallory, nothing else changed.
      await waitTile(NAMES.a);
      const room = await cameraRoom();
      expect(room.tiles.map((t) => t.sharer).sort()).toEqual(roomBefore.tiles.map((t) => t.sharer).sort());
      expect((await cameraView())?.state ?? "off").toBe("off");
    });
    expect(clientLog("b")).not.toContain(MARK);
  });

  it("AC-8.27: a client without camera support sees nothing and chat keeps working", async () => {
    // Olaf stands in for a client that knows no `mumcord.camera` (0.6.2, official Mumble).
    // The pinned test bot (mumbot 0.4.1) can still have the chat line in its send queue when
    // the script ends, and disconnect drops that queue. Hold the connection until the line
    // has been read.
    const olaf = botInBackground(["script"], { name: "Olaf", port, timeout: 60, hold: 30 });
    olaf.send(`expect message --from ${NAMES.b} --contains compat-from-bob`);
    olaf.send("say compat-from-olaf");
    olaf.process.stdin?.end();
    const finished = olaf.done.catch((e: unknown) => e);
    await on("b", async () => {
      await sessionOf("Olaf");
      await sendText("compat-from-bob");
    });
    // A's camera stayed on, and the camera wrote nothing into the chat.
    expect((await cameraView())?.state).toBe("onAir");
    // The large camera room can leave the chat outside its viewport. Close only the
    // room (not the camera) before checking the visible chat in either client.
    await $(".cam-room__bar").$("button=Close room").click();
    await waitMessage("compat-from-olaf");
    await on("b", async () => {
      await waitTile(NAMES.a);
      await $(".cam-room__bar").$("button=Close room").click();
      await waitMessage("compat-from-olaf");
    });
    expect((await cameraView())?.error ?? null).toBeNull();
    const result = await finished;
    if (result instanceof Error) throw result;
  });
});
