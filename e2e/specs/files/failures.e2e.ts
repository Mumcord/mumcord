// Spec 07 file sharing failure cases in the running client against the mumpeer test environment
// (helpers/relayenv.ts), with the debug-only test hooks of the app (relayenv.hookEnv).
// A = Alice offers, B = Bob downloads.
//
//   AC-7.11 the fileCorrupt hook on A flips a byte in transit → B's card shows the mismatch alert;
//           neither the file nor a partial file remains in B's folder.
//   AC-7.12 A changes the file on disk after offering → A's card "The file changed on disk, so
//           sharing stopped", B's card "This share ended".
//   AC-7.13 Cancel during a download → no partial file.
//   AC-7.14 Stop sharing → B's card "This share ended"; A offline → "Alice went offline"; after A
//           is back the old share stays unavailable.
//   AC-7.15 the fileExpirySeconds hook (60 s) → B's card says the share ended.
//   F3a (e) a sender that stops sending file bytes (test hook, so it stays connected) → "The
//           transfer stalled" after 30 s.
//
// Skips itself without a mumpeer checkout.
import { appendFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { trustIfAsked } from "../../helpers/chat-ui.ts";
import * as files from "../../helpers/files-ui.ts";
import { addAndConnect, completeWizard, personRow, waitConnected } from "../../helpers/voice-ui.ts";

const MB = 1024 * 1024;
const ENDED = "This share ended";

/** After a restart: connects to the saved relay server again. */
async function reconnectSaved(): Promise<void> {
  const server = $("//ul[@aria-label='Saved servers']//button[@aria-label]");
  await server.waitForClickable({ timeout: 30_000 });
  await server.click();
  await trustIfAsked();
  await waitConnected();
}

async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

/** A starts again with these test hooks and rejoins the relay server. */
async function restartAlice(hooks: relayenv.TestHooks): Promise<void> {
  await app.restart({ env: relayenv.hookEnv(hooks) });
  await reconnectSaved();
  await joinVoice();
  await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
}

/** Drops and sends files from A. */
async function offer(path: string): Promise<string> {
  const name = path.split("/").pop() ?? path;
  await files.dropFiles("a", [path]);
  await files.waitChipReady(name);
  await files.send();
  await files.waitCardText(name, "shared by you");
  return name;
}

/** B's Mumcord folder holds exactly `names` (no partial `.part` files). */
function bFolder(): string[] {
  try {
    return readdirSync(join(app.downloadsDir("b"), "Mumcord")).sort();
  } catch {
    return [];
  }
}

(relayenv.relayAvailable() ? describe : describe.skip)("file sharing: failures (spec 07)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;
  const dir = files.fileDir();

  after(async () => {
    await apps.stopB();
    // Registered names would lock later specs out: leave an empty server behind.
    await relayenv.resetRelayData();
  });

  it("setup: Alice (byte-flip hook) and Bob on a relay that allows files", async () => {
    await relayenv.resetRelayData();
    port = (await relayenv.startRelay("all")).port;
    await app.restart({ env: relayenv.hookEnv({ fileCorrupt: true }) });
    await completeWizard("Alice");
    await addAndConnect(`localhost:${String(port)}`);
    await joinVoice();
    await files.registerOnServer();
    await apps.startB();
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect(`localhost:${String(port)}`);
      await joinVoice();
      await files.registerOnServer();
    });
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await files.waitSharingReady();
  });

  it("AC-7.11: a byte changed in transit is caught and nothing is kept", async () => {
    const name = await offer(files.randomFile(dir, "flipped.dat", 2 * MB));
    await apps.onB(async () => {
      await files.download(name);
      await files.waitCardText(name, "Hash mismatch – file deleted", 120_000);
      await files.waitCardText(name, "The file changed in transit or doesn't match the offer – deleted");
      await $("//*[@role='alert'][contains(.,'Hash mismatch')]").waitForExist();
    });
    expect(bFolder()).toEqual([]);
  });

  it("AC-7.15: with a 60 s lifetime the offer ends on its own", async () => {
    await restartAlice({ fileExpirySeconds: 60 });
    const name = await offer(files.randomFile(dir, "short-lived.dat", 64 * 1024));
    await apps.onB(async () => {
      await files.waitCardText(name, "Available while Alice is online");
      await files.waitCardText(name, ENDED, 120_000);
      expect(await files.downloadBlock(name).catch(() => "none")).not.toBeNull();
    });
    await files.waitCardText(name, "The 24 hours are over", 30_000);
  });

  it("AC-7.12: a file changed on disk after the offer stops the share", async () => {
    await restartAlice({});
    const path = files.randomFile(dir, "draft.txt", 64 * 1024);
    const name = await offer(path);
    appendFileSync(path, "edited");
    await apps.onB(async () => {
      await files.download(name);
      await files.waitCardText(name, ENDED, 60_000);
    });
    await files.waitCardText(name, "The file changed on disk, so sharing stopped");
    expect(bFolder()).toEqual([]);
  });

  it("AC-7.13: Cancel during a download leaves no partial file", async () => {
    const name = await offer(files.randomFile(dir, "movie.dat", 400 * MB));
    await apps.onB(async () => {
      await files.download(name);
      await files.waitCardText(name, "% ·", 60_000);
      await files.cardButton(name, "Cancel").click();
      await files.waitCardText(name, "Download cancelled");
    });
    await browser.waitUntil(() => bFolder().length === 0, {
      timeout: 10_000,
      timeoutMsg: `partial file left: ${bFolder().join(", ")}`,
    });
  });

  it("F3a (e): a sender that stops sending gives 'The transfer stalled' after 30 s", async () => {
    // The test hook, not a paused process: freezing Alice also stops her traffic to the Mumble
    // server, whose own 30 s idle timeout can win the race on a loaded CI runner and shows the
    // other FR-7.34 text instead (issue #420). With the hook only file bytes stop, so the client's
    // stall timer is the one that ends the transfer.
    await restartAlice({ fileStallAfterBytes: 4 * MB });
    const name = await offer(files.randomFile(dir, "stalled.dat", 400 * MB));
    await apps.onB(async () => {
      const started = Date.now();
      await files.download(name);
      await files.waitCardText(name, "% ·", 60_000);
      await files.waitCardText(name, "The transfer stalled", 90_000);
      // Measured from the click, so it is a lower bound for the client's 30 s timer: the card only
      // changes after the last byte, and the UI redraws when it notices.
      expect(Date.now() - started).toBeGreaterThanOrEqual(30_000);
    });
    await browser.waitUntil(() => bFolder().length === 0, { timeout: 10_000 });
  });

  it("AC-7.14: Stop sharing ends the share for Bob", async () => {
    // Without the stall hook of the previous test, so a small file transfers normally.
    await restartAlice({});
    const name = await offer(files.randomFile(dir, "stopped.dat", 64 * 1024));
    await apps.onB(() => files.waitCardText(name, "Available while Alice is online"));
    await files.cardButton(name, "Stop sharing").click();
    await files.waitCardText(name, "You stopped sharing");
    await apps.onB(async () => {
      await files.download(name).catch(() => undefined);
      await files.waitCardText(name, ENDED, 60_000);
    });
  });

  it("AC-7.14: Alice going offline makes her offer unavailable, also after she is back", async () => {
    const name = await offer(files.randomFile(dir, "offline.dat", 64 * 1024));
    await apps.onB(() => files.waitCardText(name, "Available while Alice is online"));
    await app.restart({ env: {} });
    await apps.onB(async () => {
      await personRow("Alice").waitForExist({ reverse: true, timeout: 30_000 });
      await files.waitCardText(name, "Alice went offline. The file is no longer available.", 30_000);
    });
    await reconnectSaved();
    await joinVoice();
    await apps.onB(async () => {
      await personRow("Alice").waitForDisplayed({ timeout: 30_000 });
      await files.cardButton(name, "Download").waitForExist({ timeout: 30_000 });
      await files.download(name).catch(() => undefined);
      await files.waitCardText(name, ENDED, 60_000);
    });
  });
});
