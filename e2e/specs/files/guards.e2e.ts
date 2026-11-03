// Spec 07 file sharing guards in the running client against the mumpeer test environment
// (helpers/relayenv.ts). A = Alice offers, B = Bob receives.
//
//   AC-7.16 setup.exe and report.pdf.exe → the risky warning with "Don't download" focused; Enter
//           cancels; "Download anyway" downloads; Open on the done card asks again.
//   AC-7.17 a hand-made offer (sent by the test bot) named "../../evil\x07.txt" with a
//           right-to-left override shows as "evil_.txt" without bidi characters.
//   AC-7.19 B's maximum size 100 MB, A offers 200 MB → "Larger than your limit of 100 MB"; a file
//           over 2 GB is refused at the chip.
//   AC-7.20 B turns off Receive files → "You turned off receiving files"; B blocks A → A's new
//           offers are not shown to B.
//   AC-7.22 no relay (the relay bot stopped) → paperclip and existing cards say "This server has
//           no relay…"; the no-files relay → "…doesn't allow file sharing"; A in Direct only →
//           "File sharing needs the relay; your peer-to-peer setting is Direct only".
//   AC-7.23 private chat → no paperclip; a dropped file says "Files can only be shared in channels
//           for now".
//
// Not automated: AC-7.20's scripted `req` from a blocked person (the test bot cannot send
// mumcord.file plugin data); the engine's unit tests answer it with `gone`.
//
// Skips itself without a mumpeer checkout.
import { browser, $, expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { bot } from "../../helpers/bot.ts";
import { composer, selectChannel } from "../../helpers/chat-ui.ts";
import * as files from "../../helpers/files-ui.ts";
import { runOk } from "../../helpers/run.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const MB = 1024 * 1024;
const NO_RELAY = "This server has no relay, so file sharing isn't available.";
const NO_FILES = "This server's relay doesn't allow file sharing.";
const DIRECT_ONLY = "File sharing needs the relay; your peer-to-peer setting is Direct only.";

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

/** Drops and sends files from A. */
async function offer(paths: string[]): Promise<void> {
  await files.dropFiles("a", paths);
  for (const path of paths) await files.waitChipReady(path.split("/").pop() ?? path);
  await files.send();
}

/** The paperclip's reason (null: enabled). */
async function paperclipReason(): Promise<string | null> {
  const clip = $("button[aria-label='Share files in this channel']");
  await clip.waitForExist({ timeout: 20_000 });
  if ((await clip.getAttribute("aria-disabled")) !== "true") return null;
  return (await clip.getAttribute("data-tooltip")) ?? "";
}

async function expectPaperclip(reason: string | null): Promise<void> {
  let seen: string | null = "";
  await browser
    .waitUntil(async () => (seen = await paperclipReason()) === reason, { timeout: 60_000 })
    .catch(() => {
      throw new Error(`paperclip: want ${JSON.stringify(reason)}, got ${JSON.stringify(seen)}`);
    });
}

/** Settings → Privacy & Connections, File sharing card. */
async function fileSettings<T>(fn: () => Promise<T>): Promise<T> {
  await openSettingsGroup("Privacy & Connections");
  try {
    return await fn();
  } finally {
    await browser.keys("Escape");
  }
}

/** The label of the "Receive files" switch (the input itself is drawn by CSS). */
const receiveSwitch = () => $("//label[normalize-space(.)='Receive files from other Mumcord users']");

/** Picks the maximum file size in the open settings. */
async function pickMaxSize(label: string): Promise<void> {
  await $("#files-max-size").click();
  await $(`//*[@role='option' and normalize-space(.)='${label}']`).click();
}

/** Settings → Privacy & Connections: picks the peer-to-peer mode. */
async function setMode(mode: "directRelay" | "relayOnly" | "directOnly"): Promise<void> {
  await fileSettings(async () => {
    const radio = $(`fieldset.p2p-modes input[type='radio'][value='${mode}']`);
    await radio.waitForExist();
    await radio.parentElement().click();
    await browser.waitUntil(async () => radio.isSelected(), { timeoutMsg: `${mode} was not selected` });
  });
}

(relayenv.relayAvailable() ? describe : describe.skip)("file sharing: guards (spec 07)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;
  const dir = files.fileDir();

  after(async () => {
    await apps.stopB();
    // Registered names would lock later specs out: leave an empty server behind.
    await relayenv.resetRelayData();
  });

  it("setup: Alice and Bob in the channel of a relay that allows files", async () => {
    await relayenv.resetRelayData();
    port = (await relayenv.startRelay("all")).port;
    await connect("Alice", port);
    await apps.startB();
    await apps.onB(() => connect("Bob", port));
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await expectPaperclip(null);
  });

  it("AC-7.16: programs get the risky warning with the safe answer focused", async () => {
    const setup = files.randomFile(dir, "setup.exe", 64 * 1024);
    const report = files.randomFile(dir, "report.pdf.exe", 64 * 1024);
    await offer([setup, report]);
    await apps.onB(async () => {
      await files.waitCardText("report.pdf.exe", "Download");
      await files.cardButton("report.pdf.exe", "Download").click();
      const warning = files.dialog();
      await warning.waitForDisplayed();
      expect(await warning.getText()).toContain("This file could harm your computer");
      const focused = await browser.execute(() => document.activeElement?.textContent.trim());
      expect(focused).toBe("Don't download");
      await browser.keys("Enter");
      await warning.waitForExist({ reverse: true });
      await files.waitCardText("report.pdf.exe", "Download");
      await files.download("setup.exe");
      await files.waitDownloaded("setup.exe");
      // Opening a downloaded program asks once more.
      await files.cardButton("setup.exe", "Open").click();
      const again = files.dialog();
      await again.waitForDisplayed();
      expect(await again.getText()).toContain("setup.exe is a program or script");
      await again.$(".//button[normalize-space()=\"Don't download\" or normalize-space()='Cancel']").click();
      await again.waitForExist({ reverse: true });
    });
  });

  it("AC-7.17: a hand-made offer with a hostile name shows the sanitized name", async () => {
    const wire = JSON.stringify({
      v: 1,
      id: "0123456789abcdef0123456789abcdef",
      n: "../../evil\u0007‮txt.exe.txt",
      s: 1234,
      m: "text/plain",
      h: "a".repeat(64),
      e: Math.floor(Date.now() / 1000) + 3600,
    }).replace(/"/g, "&quot;");
    await bot(["say", `<p>Mallory shared a file</p><span data-mumcord-file="${wire}"></span>`], {
      name: "Mallory",
      port,
      timeout: 30,
    });
    await apps.onB(async () => {
      const card = $("//div[@role='group'][contains(@aria-label,'from Mallory')]");
      await card.waitForExist({ timeout: 30_000 });
      const label = (await card.getAttribute("aria-label")) ?? "";
      // No bidi controls, control characters or path separators survive.
      expect(label.split("").some((c) => /[\u202A-\u202E\u2066-\u2069/\\]/.test(c) || c.charCodeAt(0) < 32)).toBe(false);
      expect(label.startsWith("evil_")).toBe(true);
    });
  });

  it("AC-7.19: offers above the receiver's limit cannot be downloaded", async () => {
    await apps.onB(() => fileSettings(() => pickMaxSize("100 MB")));
    await offer([files.sparseFile(dir, "season.mkv", 200 * MB)]);
    await apps.onB(() => files.expectDownloadBlock("season.mkv", "Larger than your limit of 100 MB"));
  });

  it("AC-7.19: files over 2 GB are refused at the chip", async () => {
    const huge = files.sparseFile(dir, "disk.img", 2 * 1024 * MB + MB);
    await files.dropFiles("a", [huge]);
    await $("//*[normalize-space()=\"disk.img is larger than 2 GB and can't be shared\"]").waitForDisplayed({
      timeout: 20_000,
    });
  });

  it("AC-7.20: with receiving turned off, Download says so", async () => {
    await apps.onB(async () => {
      await fileSettings(async () => {
        await receiveSwitch().click();
      });
      await files.expectDownloadBlock("report.pdf.exe", "You turned off receiving files");
      await fileSettings(async () => {
        await receiveSwitch().click();
      });
    });
  });

  it("AC-7.20: once Bob blocks Alice, her new offers are not shown to him", async () => {
    await apps.onB(async () => {
      await personRow("Alice").click({ button: "right" });
      await $("//*[@role='menuitem'][normalize-space()='Block…']").click();
      await $("[data-testid='block-confirm']").click();
    });
    await offer([files.randomFile(dir, "after-block.txt", 1024)]);
    await files.waitCardText("after-block.txt", "shared by you");
    await apps.onB(async () => {
      await browser.pause(3000);
      expect(await files.card("after-block.txt").isExisting()).toBe(false);
      await personRow("Alice").click({ button: "right" });
      await $("//*[@role='menuitem'][starts-with(normalize-space(),'Unblock')]").click();
    });
  });

  it("AC-7.23: a private chat has no paperclip, and a dropped file says why", async () => {
    // A private chat needs Bob's consent first (message requests).
    await personRow("Bob").click({ button: "right" });
    await $("//*[@role='menuitem'][normalize-space()='Send message']").click();
    await $("//*[normalize-space()='Chat request to Bob']/ancestor::*[.//button[normalize-space()='Send']][1]//button[normalize-space()='Send']").click();
    await apps.onB(async () => {
      const requests = $("[data-testid='message-requests-entry']");
      await requests.waitForClickable({ timeout: 30_000 });
      await requests.click();
      await $("[data-testid='request-accept']").click();
      await requests.waitForExist({ reverse: true, timeout: 15_000 });
    });
    await personRow("Bob").click({ button: "right" });
    await $("//*[@role='menuitem'][normalize-space()='Send message']").click();
    await composer().waitForDisplayed({ timeout: 15_000 });
    await $("button[aria-label='Share files in this channel']").waitForExist({ reverse: true, timeout: 10_000 });
    await files.dropFiles("a", [files.randomFile(dir, "private.txt", 10)]);
    await $("//*[normalize-space()='Files can only be shared in channels for now']").waitForDisplayed({
      timeout: 10_000,
    });
  });

  it("AC-7.22: Direct only names the setting", async () => {
    await selectChannel("Mumpeer Test Server");
    await setMode("directOnly");
    await expectPaperclip(DIRECT_ONLY);
    await setMode("directRelay");
    await expectPaperclip(null);
  });

  it("AC-7.22: without the relay bot, paperclip and cards say the server has no relay", async () => {
    const project = process.env.MUMPEER_PROJECT ?? "mumpeer-testenv";
    const bot = await runOk("docker", [
      "ps", "-q",
      "--filter", `label=com.docker.compose.project=${project}`,
      "--filter", "label=com.docker.compose.service=mumbot",
    ]);
    await runOk("docker", ["stop", "-t", "1", bot.stdout.trim()]);
    await expectPaperclip(NO_RELAY);
    await apps.onB(async () => {
      await browser.keys("Escape");
      await selectChannel("Mumpeer Test Server");
      await files.expectDownloadBlock("report.pdf.exe", NO_RELAY);
    });
  });

  it("AC-7.2/7.22: a relay that doesn't allow files says so", async () => {
    port = (await relayenv.startRelay("no-files")).port;
    await expectPaperclip(NO_FILES);
  });
});
