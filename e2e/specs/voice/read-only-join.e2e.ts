// AC-1.35 (spec §11, FR-1.13/FR-1.14, QA.2): a fresh connect lands read-only (default channel,
// self-muted + deafened, "You're reading along…" notice with a Join button); voice only after an
// explicit Join (notice button, double-click, mumble:// link with a channel path), which restores
// the user's last own mute/deafen choice, also across an app restart (QA.7); an automatic
// reconnect after a dropped connection keeps channel and voice state.
// The app reaches testenv v15 through netsim (127.0.0.1:64800) so the TCP link can be cut; its mic
// is the file audio backend playing speech, so bots can check whether they hear the app.
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, bot, netsim, repoRoot, testenv } from "../../helpers/index.ts";
import {
  ScriptBot,
  channelName,
  composer,
  finishWizard,
  sendText,
  sleep,
} from "../../helpers/chat-ui.ts";
import {
  button,
  dialogTitle,
  fill,
  launchWithLink,
  serverButton,
  waitServerStatus,
} from "../../helpers/servers-ui.ts";
import { concat, readWav, writeWav } from "../../helpers/voice-audio.ts";
import { doubleClickRow, ownChannel, showAllPeople } from "../../helpers/voice-ui.ts";

const ME = "ReadOnlyQA";
const VIA_NETSIM = `127.0.0.1:${String(netsim.listenPort)}`;
const LINK = "mumble://localhost/Raid";
const WATCH = { name: "RoWatcher", timeout: 30 };
const OTHER = "SwitchV14";
const WATCH_OTHER = { ...WATCH, port: testenv.ports.v14 };

const dir = mkdtempSync(join(tmpdir(), "mumcord-readonly-"));
const inWav = join(dir, "in.wav");
const outWav = join(dir, "out.wav");

const muteButton = () => $("button[aria-label='Mute microphone']");
const deafenButton = () => $("button[aria-label='Deafen']");
const notice = () => $(".userpanel-readonly");
const noticeJoin = () => $(".userpanel-readonly__join");

async function pressed(el: ReturnType<typeof $>): Promise<boolean> {
  return (await el.getAttribute("aria-pressed")) === "true";
}

/** Waits until the user panel shows mute and deafen both on (`true`) or both off (`false`). */
async function waitVoiceState(readOnly: boolean): Promise<void> {
  await browser.waitUntil(
    async () =>
      (await pressed(muteButton())) === readOnly && (await pressed(deafenButton())) === readOnly,
    {
      timeout: 20_000,
      timeoutMsg: `mute/deafen never became ${readOnly ? "on" : "off"}`,
    },
  );
}

/** The read-only state as the user sees it: muted + deafened, notice with a Join button. */
async function expectReadOnlyUi(): Promise<void> {
  await waitVoiceState(true);
  await notice().waitForDisplayed({ timeout: 15_000 });
  expect(await notice().getText()).toContain("You're reading along");
  await noticeJoin().waitForDisplayed();
  expect(await noticeJoin().getText()).toMatch(/^Join /);
}

async function expectVoiceOnUi(): Promise<void> {
  await waitVoiceState(false);
  await notice().waitForExist({ reverse: true, timeout: 10_000 });
}

/** A bot in `channel` hears the app's voice (bot script: join, expect-audio). */
async function botHearsApp(channel: string, name: string): Promise<void> {
  const ear = new ScriptBot({ name, timeout: 30 });
  try {
    await ear.connected();
    if (channel !== "/") await ear.run(`join ${channel}`);
    await ear.run(`expect-audio --from ${ME} --min-rms -45 --timeout 25`, 40_000);
  } finally {
    ear.stop();
  }
}

/** A bot in the default channel (where it lands) hears nothing from the app for a while. */
async function botHearsNothing(): Promise<void> {
  let heard = false;
  try {
    await bot(["expect-audio", "--from", ME, "--min-rms", "-55", "--timeout", "8"], {
      name: "RoEar",
      timeout: 20,
    });
    heard = true;
  } catch {
    // Timed out without voice from the app: expected.
  }
  expect(heard).toBe(false);
}

/** A bot in `channel` hears nothing from the app for a while. */
async function botInChannelHearsNothing(channel: string): Promise<void> {
  const ear = new ScriptBot({ name: "RoEar", timeout: 30 });
  let heard = false;
  try {
    await ear.connected();
    await ear.run(`join ${channel}`);
    try {
      await ear.run(`expect-audio --from ${ME} --min-rms -55 --timeout 8`, 20_000);
      heard = true;
    } catch {
      // Timed out without voice from the app: expected.
    }
  } finally {
    ear.stop();
  }
  expect(heard).toBe(false);
}

/** Waits until no helper bot is listed in the tree, so a double-click lands on a settled layout. */
async function treeSettled(): Promise<void> {
  await showAllPeople();
  await browser.waitUntil(
    async () =>
      !(await browser.execute(() =>
        Array.from(document.querySelectorAll("[role='tree'] .tree-person__name")).some((n) =>
          /^Ro(Ear|Watcher|Reader)/.test(n.textContent),
        ),
      )),
    { timeout: 15_000, timeoutMsg: "helper bots still listed" },
  );
  await sleep(500);
}

/** Connects to the saved netsim server from the server bar (fresh, user-initiated connect). */
async function connectSaved(): Promise<void> {
  const server = $(`//ul[@aria-label='Saved servers']//button[starts-with(@aria-label,'127.0.0.1')]`);
  await server.waitForClickable({ timeout: 30_000 });
  await server.click();
  await waitConnected();
}

async function waitConnected(): Promise<void> {
  await $("//button[contains(@aria-label,'(connected)')]").waitForExist({ timeout: 30_000 });
  await channelName("Raid").waitForDisplayed({ timeout: 15_000 });
  await showAllPeople();
}

/** Clicks whichever of the given dialog buttons shows up until `done` resolves true. */
async function clickThrough(buttons: string[], done: () => Promise<boolean>): Promise<void> {
  const deadline = Date.now() + 60_000;
  while (!(await done())) {
    if (Date.now() > deadline) throw new Error(`never finished; buttons ${buttons.join(", ")}`);
    for (const text of buttons) {
      const b = $(`//button[normalize-space(.)='${text}']`);
      if ((await b.isExisting()) && (await b.isClickable())) await b.click();
    }
    await sleep(500);
  }
}

describe("read-only connect and explicit join (AC-1.35)", () => {
  before(async () => {
    const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
    // ~10 minutes of speech (the file backend plays silence after the end).
    writeWav(inWav, concat(...Array.from({ length: 100 }, () => speech)));
    await testenv.up("v15");
    await testenv.up("v14");
    await netsim.start("v15");
    await app.restart({ audio: `file:${inWav}:${outWav}` });
  });

  after(async () => {
    await netsim.ctl("reset").catch(() => undefined);
    netsim.stop();
  });

  it("first connect lands read-only in the default channel; the bot sees the user deafened", async () => {
    await finishWizard(ME);
    const input = $("//label[.='Server address or mumble:// link']/following::input[1]");
    await input.waitForDisplayed({ timeout: 30_000 });
    await input.setValue(VIA_NETSIM);
    await $("//button[normalize-space(.)='Add and connect']").click();
    await $("//button[.='Trust and connect']").waitForDisplayed({ timeout: 20_000 });
    await $("//button[.='Trust and connect']").click();
    await waitConnected();
    await expectReadOnlyUi();
    // Mumble servers report a self-deafened user as deafened only (deafen implies mute), so the
    // bot checks the deafen flag and that nothing is heard.
    await bot(["expect", "user", ME, "--in", "/", "--deaf", "true"], WATCH);
    await botHearsNothing();
  });

  it("chat works while reading along", async () => {
    const reader = new ScriptBot({ name: "RoReader", timeout: 30 });
    try {
      await reader.connected();
      const check = reader.run(`expect message --from ${ME} --contains ro-chat-35`, 40_000);
      await composer().waitForEnabled({ timeout: 15_000 });
      await sendText("ro-chat-35");
      await check;
    } finally {
      reader.stop();
    }
  });

  it("the notice's Join button turns voice on; the bot hears the app", async () => {
    await noticeJoin().click();
    await expectVoiceOnUi();
    await bot(["expect", "user", ME, "--in", "/", "--deaf", "false", "--muted", "false"], WATCH);
    await botHearsApp("/", "RoEar");
  });

  it("double-clicking Raid moves there with voice on", async () => {
    await treeSettled();
    await doubleClickRow(channelName("Raid"));
    await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false", "--muted", "false"], WATCH);
    await browser.waitUntil(async () => (await ownChannel()) === "Raid", {
      timeout: 15_000,
      timeoutMsg: "app tree does not show the user in Raid",
    });
    await expectVoiceOnUi();
    await botHearsApp("Raid", "RoEar");
  });

  it("app restart: fresh connect lands read-only in the default channel, not in Raid", async () => {
    await app.restart();
    await connectSaved();
    await expectReadOnlyUi();
    await bot(["expect", "user", ME, "--in", "/", "--deaf", "true"], WATCH);
    expect(await ownChannel()).not.toBe("Raid");
    await botHearsNothing();
  });

  it("double-click from read-only joins Raid with voice on; the bot hears the app", async () => {
    await treeSettled();
    await doubleClickRow(channelName("Raid"));
    await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false", "--muted", "false"], WATCH);
    await expectVoiceOnUi();
    await botHearsApp("Raid", "RoEar");
  });

  it("automatic reconnect after a dropped TCP link keeps channel and voice on", async () => {
    await netsim.ctl("kill-tcp");
    await $(".servers-status*=Reconnecting").waitForDisplayed({ timeout: 30_000, interval: 50 });
    await $(".servers-status").waitForExist({ reverse: true, timeout: 90_000 });
    await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false", "--muted", "false"], WATCH);
    await expectVoiceOnUi();
    await botHearsApp("Raid", "RoEar");
  });

  it("automatic reconnect keeps a self-mute (voice state as before the drop)", async () => {
    await muteButton().click();
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"], WATCH);
    await netsim.ctl("kill-tcp");
    await $(".servers-status*=Reconnecting").waitForDisplayed({ timeout: 30_000, interval: 50 });
    await $(".servers-status").waitForExist({ reverse: true, timeout: 90_000 });
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"], WATCH);
    // The own user state after a reconnect can reach the UI just after the bot sees it.
    await browser.waitUntil(
      async () => (await pressed(muteButton())) && !(await pressed(deafenButton())),
      { timeout: 15_000, timeoutMsg: "user panel does not show muted after the reconnect" },
    );
    await notice().waitForExist({ reverse: true, timeout: 5_000 });
    await muteButton().click();
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "false", "--deaf", "false"], WATCH);
  });

  it("muted before the restart: join from read-only keeps the mute, not deafened (QA.7)", async () => {
    await muteButton().click();
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"], WATCH);
    await app.restart();
    await connectSaved();
    await expectReadOnlyUi();
    await treeSettled();
    await doubleClickRow(channelName("Raid"));
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"], WATCH);
    await browser.waitUntil(
      async () => (await pressed(muteButton())) && !(await pressed(deafenButton())),
      { timeout: 15_000, timeoutMsg: "user panel does not show muted, not deafened" },
    );
    await notice().waitForExist({ reverse: true, timeout: 10_000 });
    await botInChannelHearsNothing("Raid");
    // Unmuting while joined is the new own choice; the bot hears the app again.
    await muteButton().click();
    await expectVoiceOnUi();
    await botHearsApp("Raid", "RoEar");
  });

  it("a mumble:// link with a channel path joins that channel with voice on", async () => {
    // Back to read-only first, so the link's join is what turns voice on.
    await app.restart();
    await connectSaved();
    await expectReadOnlyUi();
    // Like a browser handing the link to the registered handler: a second launch forwards it.
    const second = launchWithLink(LINK);
    await new Promise<void>((resolve) => {
      second.once("exit", () => {
        resolve();
      });
    });
    await clickThrough(["Save and connect", "Disconnect and switch", "Trust and connect"], () =>
      bot(["expect", "user", ME, "--in", "Raid", "--deaf", "false", "--muted", "false"], {
        name: "RoWatcher",
        timeout: 3,
      }).then(
        () => true,
        () => false,
      ),
    );
    await expectVoiceOnUi();
    await botHearsApp("Raid", "RoEar");
  });

  it("server switch with voice on: Join on the other server restores the own voice (QA.7)", async () => {
    // Deafened while joined, then voice on again by undeafening the read-only state of the next
    // connect: the undeafen, not the earlier deafen, is the own choice a Join restores.
    await deafenButton().click();
    await bot(["expect", "user", ME, "--in", "Raid", "--deaf", "true"], WATCH);
    await $("button[aria-label^='Disconnect from']").click();
    await $("//button[contains(@aria-label,'(connected)')]").waitForExist({
      reverse: true,
      timeout: 15_000,
    });
    await connectSaved();
    await expectReadOnlyUi();
    await deafenButton().click();
    await expectVoiceOnUi();
    await bot(["expect", "user", ME, "--deaf", "false", "--muted", "false"], WATCH);

    await browser.$('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed();
    await fill("Address", "127.0.0.1");
    await fill("Port", String(testenv.ports.v14));
    await fill("Name in the server bar", OTHER);
    await fill("Your name on this server", ME);
    await button("Add server").click();
    await serverButton(OTHER).click();
    await dialogTitle("Switch server?").waitForDisplayed();
    await button("Disconnect and switch").click();
    await clickThrough(["Trust and connect"], async () =>
      (await serverButton(OTHER).getAttribute("aria-label"))?.endsWith("(connected)") ?? false,
    );
    await channelName("Raid").waitForDisplayed({ timeout: 15_000 });
    await expectReadOnlyUi();
    await bot(["expect", "user", ME, "--deaf", "true"], WATCH_OTHER);
    await treeSettled();
    await doubleClickRow(channelName("Raid"));
    await bot(
      ["expect", "user", ME, "--in", "Raid", "--deaf", "false", "--muted", "false"],
      WATCH_OTHER,
    );
    await expectVoiceOnUi();

    // Muted there, then switch back: the Join restores muted only.
    await muteButton().click();
    await bot(
      ["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"],
      WATCH_OTHER,
    );
    await serverButton("127.0.0.1").click();
    await dialogTitle("Switch server?").waitForDisplayed();
    await button("Disconnect and switch").click();
    await waitServerStatus(OTHER, null);
    await waitConnected();
    await expectReadOnlyUi();
    await treeSettled();
    await doubleClickRow(channelName("Raid"));
    await bot(["expect", "user", ME, "--in", "Raid", "--muted", "true", "--deaf", "false"], WATCH);
    await browser.waitUntil(
      async () => (await pressed(muteButton())) && !(await pressed(deafenButton())),
      { timeout: 15_000, timeoutMsg: "user panel does not show muted, not deafened" },
    );
    await notice().waitForExist({ reverse: true, timeout: 10_000 });
  });
});
