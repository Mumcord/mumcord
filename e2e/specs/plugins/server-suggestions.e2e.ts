// AC-6.26 (spec 06, FR-6.77 … FR-6.79) with the real mumbot `plugins` module: a registered bot
// announces Dice; the client shows a notification-centre entry and no OS notification (the
// container has no notification service, so an OS notification would fall back to an in-app
// toast); Not now hides it until the bot's `seq` changes; Install from the entry installs the
// plugin; Don't ask again for this server silences every later list. The unregistered sender
// ("sender not registered" in the inspector) is covered by the engine's live test
// `plugin_messages_flow_between_engines_until_blocked` (crates/engine/tests/live_plugins.rs).
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import { addFirstServer, finishWizard } from "../../helpers/chat-ui.ts";
import { app, bin, registry, testenv } from "../../helpers/index.ts";
import { runOk } from "../../helpers/run.ts";
import { certFingerprint } from "../../helpers/servers-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";

interface Announce {
  id: string;
  for: string;
}

const DICE: Announce = { id: "dice", for: "board game nights" };
const POLL: Announce = { id: "poll", for: "votes on the raid time" };
const PETS: Announce = { id: "echolings", for: "pets in the lobby" };

/** A registered mumbot that runs only the `plugins` module (`mumbot plugins --config`). */
class PluginsBot {
  private child: ChildProcess | undefined;
  private log = "";

  constructor(
    private readonly dir: string,
    private readonly name: string,
  ) {}

  private env(): NodeJS.ProcessEnv {
    return { ...process.env, MUMBOT_DATA: this.dir, RUST_LOG: "mumbot=debug" };
  }

  async register(): Promise<void> {
    const args = ["--name", this.name, "--port", String(testenv.ports.v15), "register"];
    // A freshly started server may not accept connections yet.
    for (let attempt = 1; ; attempt++) {
      try {
        await runOk(bin.bot, args, this.env());
        return;
      } catch (error) {
        if (attempt === 5) throw error;
        await new Promise((resolve) => setTimeout(resolve, 2_000));
      }
    }
  }

  /** (Re)starts the bot with this list; a different list raises `seq` (plugins-seq.json). */
  async start(list: readonly Announce[]): Promise<void> {
    this.stop();
    const entries = list
      .map((p) => `[[plugins.announce]]\nid = "${p.id}"\nmin = "0.1.0"\nsource = "official"\nfor = "${p.for}"\n`)
      .join("\n");
    const config = join(this.dir, "bot.toml");
    writeFileSync(
      config,
      `[server]\nhost = "127.0.0.1"\nport = ${String(testenv.ports.v15)}\nname = "${this.name}"\n` +
        `data_dir = "${this.dir}"\nserver_cert_sha256 = "${await certFingerprint("A")}"\n\n${entries}`,
    );
    this.log = "";
    const child = spawn(bin.bot, ["plugins", "--config", config], { env: this.env() });
    child.stdout.on("data", (d: Buffer) => (this.log += d.toString()));
    child.stderr.on("data", (d: Buffer) => (this.log += d.toString()));
    this.child = child;
  }

  /** Waits until the bot has sent its list to a client (proof for the "nothing shows" checks). */
  async announced(): Promise<void> {
    await browser.waitUntil(async () => Promise.resolve(this.log.includes("plugins: list announced")), {
      timeout: 30_000,
      timeoutMsg: `the plugins bot never announced its list:\n${this.log}`,
    });
  }

  stop(): void {
    this.child?.kill("SIGTERM");
    this.child = undefined;
  }
}

function bell() {
  return $("[data-testid='notification-bell']");
}

function centre() {
  return $("[data-testid='notification-centre']");
}

async function openCentre(): Promise<void> {
  if (await centre().isExisting()) return;
  await bell().click();
  await centre().waitForDisplayed();
}

async function closeCentre(): Promise<void> {
  if (!(await centre().isExisting())) return;
  await bell().click();
  await centre().waitForExist({ reverse: true });
}

/** The centre entry of one suggestion, found by the bot's "for" text. */
function entry(p: Announce) {
  return $(`//section[@data-testid='notification-centre']//li[contains(@class,'centre-entry')][contains(., '${p.for}')]`);
}

function entryButton(p: Announce, testId: string) {
  return entry(p).$(`.//button[@data-testid='${testId}']`);
}

async function suggestionEntries(): Promise<number> {
  await openCentre();
  return (await $$("//section[@data-testid='notification-centre']//button[@data-testid='centre-plugin-not-now']"))
    .length;
}

/** No suggestion arrives as an in-app toast, which is where an OS notification would land. */
async function expectNoToast(): Promise<void> {
  const toasts = await $$("//section[@aria-label='Notifications']//*[contains(@class,'toast')]");
  for (const toast of toasts) {
    expect(await toast.getText()).not.toContain("This server uses");
  }
}

describe("plugins: server suggestions from mumbot (AC-6.26)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;
  let dir: string;
  let mumbot: PluginsBot;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
    dir = mkdtempSync(join(tmpdir(), "mumcord-srvplugins-"));
    // A fresh name per run: the server keeps registrations between runs.
    mumbot = new PluginsBot(dir, `Plugins${String(Date.now() % 100_000)}`);
    await mumbot.register();
    await app.restart({ env: fixtures.env() });
    await finishWizard("Alice");
    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74); suggestions resolve against that official list.
    await openSettingsGroup("Advanced & Developer");
    const developerMode = settingsCard("Developer mode").$("input[role='switch']");
    await developerMode.waitForExist({ timeout: 10_000 });
    if (!(await developerMode.isSelected())) await developerMode.click();
    await expect(developerMode).toBeSelected({ wait: 5_000 });
    await browser.keys(["Escape"]);
    await addFirstServer(testenv.ports.v15);
  });

  after(async () => {
    mumbot.stop();
    rmSync(dir, { recursive: true, force: true });
    await fixtures.stop();
  });

  it("shows Dice in the notification centre, without an OS notification", async () => {
    await mumbot.start([DICE]);
    await mumbot.announced();
    await browser.waitUntil(async () => /unread/.test((await bell().getAttribute("aria-label")) ?? ""), {
      timeout: 30_000,
      timeoutMsg: "no unread entry after the bot announced Dice",
    });
    await expectNoToast();
    await openCentre();
    await expect(entry(DICE)).toHaveText(expect.stringContaining("This server uses"));
    for (const id of ["centre-plugin-install", "centre-plugin-details", "centre-plugin-not-now", "centre-plugin-dont-ask"]) {
      await expect(entryButton(DICE, id)).toBeDisplayed();
    }
  });

  it("Not now hides Dice until the bot's list changes", async () => {
    await entryButton(DICE, "centre-plugin-not-now").click();
    await entry(DICE).waitForExist({ reverse: true, timeout: 10_000 });
    await closeCentre();

    // Same list again: same seq, so nothing comes back.
    await mumbot.start([DICE]);
    await mumbot.announced();
    await browser.pause(3_000);
    expect(await suggestionEntries()).toBe(0);
    await closeCentre();

    // A changed list raises seq: Dice returns, Poll is new.
    await mumbot.start([DICE, POLL]);
    await openCentre();
    await entry(DICE).waitForDisplayed({ timeout: 30_000 });
    await entry(POLL).waitForDisplayed({ timeout: 10_000 });
    await expectNoToast();
  });

  it("installs Dice from its entry", async () => {
    await entryButton(DICE, "centre-plugin-install").click();
    const confirm = $(
      "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]//button[normalize-space(.)='Install']",
    );
    // The prompt names the server's suggestion and its reason (FR-6.79).
    await expect($("//div[contains(concat(' ',@class,' '),' plugin-prompt ')]")).toHaveText(
      expect.stringContaining(DICE.for),
      { wait: 30_000 },
    );
    // The permission list pushes Install below the fold of the dialog.
    await confirm.waitForExist({ timeout: 60_000 });
    await confirm.scrollIntoView();
    await confirm.waitForClickable({ timeout: 60_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining("installed"), { wait: 60_000 });
    await browser.keys(["Escape"]);
  });

  it("Don't ask again silences every later list of this server", async () => {
    await openCentre();
    await entryButton(POLL, "centre-plugin-dont-ask").click();
    await browser.waitUntil(async () => (await suggestionEntries()) === 0, {
      timeout: 10_000,
      timeoutMsg: "suggestions still shown after Don't ask again",
    });
    await closeCentre();

    await mumbot.start([POLL, PETS]);
    await mumbot.announced();
    await browser.pause(3_000);
    expect(await suggestionEntries()).toBe(0);
  });
});
