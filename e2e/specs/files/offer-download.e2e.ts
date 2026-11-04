// Spec 07 file sharing, the happy path in the running client against the mumpeer test environment
// (helpers/relayenv.ts): A = Alice offers, B = Bob and C = Carol download, O = a plain Mumble client
// (the test bot) that only sees the fallback line.
//
//   AC-7.6  a real drag of a file onto the channel chat → overlay, chip, Send → A's own card, B and
//           C see a file card, O sees "Alice shared a file: … Download it with Mumcord."
//   AC-7.7  Download → confirmation with full checksum and the default folder <Downloads>/Mumcord →
//           progress → "✓ Downloaded" + "✓ Matches the offer"; same SHA-256 on disk; the relay's
//           log shows the file session; the file carries the Linux mark-of-the-web (AC-7.18).
//   F3a (a) a 3 MB file arrives byte-exact through the relay.
//   F3a (c) Alice watches Bob's screen share over a direct connection; downloading a file from Bob
//           moves the link to the relay, and the share keeps running.
//
// Not automated here: AC-7.7's tcpdump in a network namespace (needs root, netns.sh).
//
// Skips itself without a mumpeer checkout.
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { botInBackground, type BackgroundBot } from "../../helpers/bot.ts";
import { startScreenShare } from "../../helpers/chat-ui.ts";
import * as files from "../../helpers/files-ui.ts";
import { run } from "../../helpers/run.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const MB = 1024 * 1024;

/** Whether the relay carried a session (its /v1/status counts open ones; the link stays a while). */
async function relayUsed(): Promise<boolean> {
  return ((await relayenv.relayStatus()).sessions as number) > 0;
}

/** Joins the shown server's channel when its read-only notice offers it. */
async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

async function connect(name: string, port: number): Promise<void> {
  await completeWizard(name);
  await addAndConnect(`localhost:${String(port)}`);
  await joinVoice();
  await files.registerOnServer();
}

(relayenv.relayAvailable() ? describe : describe.skip)("file sharing: offer and download (spec 07)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;
  let plain: BackgroundBot | undefined;
  let plainOut = "";
  const dir = files.fileDir();
  let big = "";
  let small = "";

  after(async () => {
    plain?.stop();
    await apps.stopC();
    await apps.stopB();
    // Registered names would lock later specs out: leave an empty server behind.
    await relayenv.resetRelayData();
  });

  it("setup: Alice, Bob and Carol in the channel of a relay that allows files", async () => {
    await relayenv.resetRelayData();
    port = (await relayenv.startRelay("all")).port;
    // O, a plain Mumble client, listens for the fallback line from the start.
    plain = botInBackground(["script"], { name: "Olga", port, timeout: 300 });
    plain.process.stdout?.on("data", (d: Buffer) => (plainOut += d.toString()));
    plain.send("expect message --from Alice --contains 'Download it with Mumcord.'");
    await connect("Alice", port);
    await apps.startB();
    await apps.onB(() => connect("Bob", port));
    await apps.startC();
    await apps.onC(() => connect("Carol", port));
    for (const name of ["Bob", "Carol", "Olga"])
      await personRow(name).waitForDisplayed({ timeout: 30_000 });
    await files.waitSharingReady();
    big = files.randomFile(dir, "game-night.dat", 50 * MB);
    small = files.randomFile(dir, "notes.dat", 3 * MB);
  });

  it("default folder: received files go to <Downloads>/Mumcord", async () => {
    await apps.onB(async () => {
      await openSettingsGroup("Privacy & Connections");
      const folder = $("#files-save-folder");
      await folder.waitForDisplayed();
      expect(await folder.getProperty("textContent")).toBe(join(app.downloadsDir("b"), "Mumcord"));
      await browser.keys("Escape");
    });
  });

  it("AC-7.6: dragging a file over the chat shows the drop overlay", async () => {
    let title = "";
    await Promise.all([
      files.dropFiles("a", [big], true),
      (async () => {
        await $(".file-drop").waitForDisplayed({ timeout: 15_000, timeoutMsg: "no drop overlay" });
        title = await $(".file-drop__title").getText();
      })(),
    ]);
    expect(title).toMatch(/^Drop to share with #/);
    await $(".file-drop").waitForExist({ reverse: true, timeout: 10_000 });
  });

  it("AC-7.6: a dropped file becomes a chip, and Send posts the offer", async () => {
    await files.dropFiles("a", [big, small]);
    await files.waitChipReady("game-night.dat");
    await files.waitChipReady("notes.dat");
    await files.send();
    await files.waitCardText("game-night.dat", "50.0 MB · File · shared by you");
    await files.waitCardText("game-night.dat", "shared by you");
    await files.waitCardText("game-night.dat", "Sharing while you are online · ends");
    await files.waitCardText("game-night.dat", "Stop sharing");
  });

  it("AC-7.6: Bob and Carol see a file card, a plain Mumble client the fallback line", async () => {
    for (const on of [apps.onB, apps.onC]) {
      await on(async () => {
        await files.waitCardText("game-night.dat", "Available while Alice is online");
        await files.cardButton("game-night.dat", "Download").waitForDisplayed();
      });
    }
    await browser.waitUntil(() => plainOut.includes("ok expect message"), {
      timeout: 30_000,
      timeoutMsg: `the plain Mumble client never saw the fallback line (${plainOut})`,
    });
  });

  it("AC-7.9/7.10: one transfer at a time: Carol queues behind Bob, Bob's other download waits", async () => {
    await files.dropFiles("a", [files.sparseFile(dir, "season.dat", 600 * MB)]);
    await files.waitChipReady("season.dat");
    await files.send();
    await files.waitCardText("season.dat", "shared by you");
    await apps.onB(() => files.download("season.dat"));
    await apps.onB(() => files.waitCardText("season.dat", "% ·", 60_000));
    await apps.onC(() => files.download("season.dat"));
    await apps.onC(() => files.waitCardText("season.dat", "Queued (position 1)", 30_000));
    await files.waitCardText("season.dat", "1 download in progress · Bob");
    await files.waitCardText("season.dat", "1 waiting");
    await apps.onB(() => files.expectDownloadBlock("notes.dat", "Another download is running"));
  });

  it("AC-7.13: Bob cancels: no partial file, and Carol's queued download starts", async () => {
    await apps.onB(async () => {
      await files.cardButton("season.dat", "Cancel").click();
      await files.waitCardText("season.dat", "Download cancelled");
      await files.expectDownloadBlock("notes.dat", null);
    });
    expect(readdirSync(join(app.downloadsDir("b"), "Mumcord"))).toEqual([]);
    await apps.onC(async () => {
      await files.waitCardText("season.dat", "% ·", 60_000);
      await files.cardButton("season.dat", "Cancel").click();
      await files.waitCardText("season.dat", "Download cancelled");
    });
  });

  it("AC-7.7: Bob downloads after confirming checksum and folder; the file matches", async () => {
    const hash = await files.sha256(big);
    await apps.onB(async () => {
      await files.cardButton("game-night.dat", "Download").click();
      const confirm = files.dialog();
      await confirm.waitForDisplayed();
      const text = (await confirm.getText()).replace(/\s+/g, " ");
      expect(text).toContain(hash);
      expect(text).toContain(join(app.downloadsDir("b"), "Mumcord"));
      await confirm.$(".//button[normalize-space()='Download']").click();
      await files.waitDownloaded("game-night.dat");
    });
    const saved = join(app.downloadsDir("b"), "Mumcord", "game-night.dat");
    expect(await files.sha256(saved)).toBe(hash);
    expect(readdirSync(join(app.downloadsDir("b"), "Mumcord"))).toEqual(["game-night.dat"]);
    expect(await relayUsed()).toBe(true);
    const motw = await run("python3", [
      "-I",
      "-c",
      "import os, sys; print(os.getxattr(sys.argv[1], 'user.xdg.origin.url').decode())",
      saved,
    ]);
    // AC-7.18 on Linux: the origin URL names the server (where the file system keeps xattrs).
    if (motw.code === 0) expect(motw.stdout).toMatch(/^mumble:\/\/localhost/);
    else console.warn(`[files] no user xattrs here: ${motw.stderr}`);
  });

  it("F3a (a): a 3 MB file arrives byte-exact through the relay", async () => {
    await apps.onB(async () => {
      await files.download("notes.dat");
      await files.waitDownloaded("notes.dat");
    });
    const saved = join(app.downloadsDir("b"), "Mumcord", "notes.dat");
    expect(await files.sha256(saved)).toBe(await files.sha256(small));
  });

  it("F3a (c): a watched screen share keeps running when a download moves the link to the relay", async () => {
    await apps.onB(startScreenShare);
    const watch = $(
      "//div[contains(@class,'activity-banner')][contains(.,'is sharing their screen')]//button[normalize-space()='Watch']",
    );
    await watch.waitForClickable({ timeout: 30_000 });
    await watch.click();
    const picture = $("[aria-label='Shared screen of Bob']");
    await picture.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Bob's screen never showed" });
    const clip = files.randomFile(dir, "clip.dat", 3 * MB);
    await apps.onB(async () => {
      await files.dropFiles("b", [clip]);
      await files.waitChipReady("clip.dat");
      await files.send();
    });
    await files.download("clip.dat");
    await files.waitDownloaded("clip.dat");
    expect(await files.sha256(join(app.downloadsDir("a"), "Mumcord", "clip.dat"))).toBe(
      await files.sha256(clip),
    );
    await browser.pause(5000);
    expect(await picture.isDisplayed()).toBe(true);
    for (const broken of ["could not be opened", "Couldn't reach Bob"])
      expect(await $(`//*[contains(normalize-space(),"${broken}")]`).isExisting()).toBe(false);
  });
});
