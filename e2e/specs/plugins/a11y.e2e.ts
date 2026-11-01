// AC-6.29 (spec 06, C14): accessibility of the plugin UI in the running client. axe (WCAG 2.2 AA,
// colour contrast included) on the Plugins strip, the install prompt with its progress bar, every
// Settings → Plugins tab, the Poll card with "Show original text", the Mumbot Music dialog and the
// EVE fit history panel, in the light and the dark theme; a keyboard-only Poll walkthrough; the
// screen-reader text of replaced and hidden chat lines (F46, live region); cards at a 150 % font
// size (F44); reduced motion freezes Echolings; German permission texts.
// The bot "Carol" stands in for a plain Mumble user; "Tunes" is a local mumbot music module.
import { type ChildProcess, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, $$, browser, expect } from "@wdio/globals";
import type { Result } from "axe-core";
import { app, axe, bin, registry, repoRoot, testenv } from "../../helpers/index.ts";
import { composer, joinChannel, ScriptBot, selectChannel } from "../../helpers/chat-ui.ts";
import { runOk } from "../../helpers/run.ts";
import { certFingerprint } from "../../helpers/servers-ui.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, personRow, reconnect } from "../../helpers/voice-ui.ts";
import { chooseLanguage, describeFocus, focus, pressUntil, tabTo, typeText } from "../app/flows.ts";

type Element = ReturnType<typeof $>;

const CHANNEL = "Lobby";
const QUESTION = "Pizza or pasta?";
const FIT = ["[Rifter, Tackle]", "Damage Control II", "Warp Scrambler II"];
/** A fresh name per run: the server keeps registrations between runs. */
const MUSIC_BOT = `Tunes${String(Date.now() % 100_000)}`;
const PLUGIN_TABS = ["Installed", "Browse", "Updates", "Repositories", "Developer"];

const prompt = "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]";
const pluginTab = (name: string) =>
  // Installed carries its count ("Installed 3").
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const sheet = () => $(".pstrip-sheet");
const dialog = () => $("//div[@role='dialog' and contains(@class,'userpanel-modal__dialog')]");
const dialogField = (label: string) =>
  dialog().$(`.//label[normalize-space(.)='${label}']/following::input[1]`);
const cardItem = (plugin: string) =>
  $(`(//li[.//section[contains(@class,'plugin-chat-card')][@aria-label='Card from ${plugin}']])[last()]`);

/** Every violation of one screen, collected so a run reports all screens at once. */
const findings: string[] = [];

async function check(label: string, include: string[]): Promise<void> {
  const violations: Result[] = await axe.scan(label, {}, { include: include.map((s) => [s]) });
  for (const v of violations) {
    const targets = v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(" | ");
    findings.push(`${label}: ${v.impact ?? "?"} ${v.id} (${v.help}) → ${targets}`);
  }
}

function expectNoFindings(): void {
  const all = findings.splice(0);
  expect(all.join("\n")).toBe("");
}

async function waitText(el: () => Element, text: string, timeout = 20_000): Promise<void> {
  await browser.waitUntil(
    async () => (await el().getProperty("textContent").then(String)).includes(text),
    { timeout, timeoutMsg: `${JSON.stringify(text)} never showed` },
  );
}

/** Puts `value` into a React text field (typed keys lose Shift in some WebKitWebDriver runs). */
async function setField(input: Element, value: string): Promise<void> {
  await input.waitForDisplayed({ timeout: 10_000 });
  await browser.execute(
    (el: HTMLInputElement, text: string) => {
      el.focus();
      Reflect.set(HTMLInputElement.prototype, "value", text, el);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      // Some plugin fields commit on leaving the field.
      el.blur();
    },
    (await input.getElement()) as unknown as HTMLInputElement,
    value,
  );
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

async function developerMode(): Promise<void> {
  await openSettingsGroup("Advanced & Developer");
  const developer = settingsCard("Developer mode").$("input[role='switch']");
  await developer.waitForExist({ timeout: 10_000 });
  if (!(await developer.isSelected())) await developer.click();
  await expect(developer).toBeSelected({ wait: 5_000 });
}

/** Scans every Settings → Plugins tab. */
async function checkPluginSettings(theme: string): Promise<void> {
  await openSettingsGroup("Plugins");
  for (const name of PLUGIN_TABS) {
    await pluginTab(name).click();
    await browser.waitUntil(async () => (await pluginTab(name).getAttribute("aria-selected")) === "true");
    await browser.pause(500);
    await check(`${theme}-settings-${name}`, [".overlay__dialog"]);
  }
}

/**
 * Runs axe on the progress bar the moment it shows (it lasts well under a second for a local
 * registry, too short for a WebDriver round trip) and keeps the result in the page.
 */
async function armProgressScan(): Promise<void> {
  await axe.inject();
  await browser.execute((tags: string[]) => {
    const w = window as unknown as Record<string, unknown>;
    w.__c14Progress = null;
    const observer = new MutationObserver(() => {
      if (document.querySelector(".plugin-progress") === null) return;
      observer.disconnect();
      const run = (w.axe as { run: (c: unknown, o: unknown) => Promise<{ violations: unknown[] }> }).run;
      run({ include: [[".plugin-progress"]] }, { runOnly: { type: "tag", values: tags } })
        .then((r) => (w.__c14Progress = { violations: r.violations }))
        .catch((e: unknown) => (w.__c14Progress = { error: String(e) }));
    });
    observer.observe(document.body, { childList: true, subtree: true });
  }, axe.wcag22aa);
}

async function progressFindings(label: string): Promise<void> {
  const result = await browser.waitUntil(
    () => browser.execute(() => (window as unknown as Record<string, unknown>).__c14Progress),
    { timeout: 30_000, timeoutMsg: "the progress bar never showed" },
  );
  const r = result as { error?: string; violations?: { id: string; impact: string }[] };
  if (r.error !== undefined) findings.push(`${label}: axe failed on the progress bar: ${r.error}`);
  for (const v of r.violations ?? []) findings.push(`${label}: ${v.impact} ${v.id}`);
}

/** Installs `name` from Browse with every optional permission on; scans the prompt once. */
async function install(name: string, scanAs?: string): Promise<void> {
  await pluginTab("Browse").click();
  const button = $(`button[aria-label='Install ${name}']`);
  await button.waitForExist({ timeout: 30_000 });
  await button.scrollIntoView({ block: "center" });
  await button.waitForClickable({ timeout: 30_000 });
  if (scanAs !== undefined) await armProgressScan();
  await button.click();
  const confirm = $(`${prompt}//button[normalize-space(.)='Install']`);
  await confirm.waitForClickable({ timeout: 60_000 });
  if (scanAs !== undefined) {
    await progressFindings(`${scanAs}-progress`);
    await check(`${scanAs}-prompt`, [".plugin-prompt"]);
  }
  for (const box of await $$(`${prompt}//label[contains(@class,'perm-label')]//input[@type='checkbox']`)) {
    if (!(await box.isSelected())) await box.click();
  }
  await confirm.click();
  await expect($(".plugin-toast")).toHaveText(expect.stringContaining(`${name} installed`), {
    wait: 60_000,
  });
}

/** Opens a plugin's inline panel in the strip (no-op when it is open). */
async function openPanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  await icon.waitForClickable({ timeout: 30_000 });
  // The strip remembers its open panel across restarts, so its state decides the clicks needed.
  for (let i = 0; i < 3; i++) {
    const open = (await icon.getAttribute("aria-pressed")) === "true";
    if (open && (await $(".pstrip-panel").isDisplayed())) return;
    await icon.click();
    await $(".pstrip-panel").waitForDisplayed({ timeout: 3_000 }).catch(() => undefined);
  }
  const state = await browser.execute(() => document.querySelector(".pstrip")?.outerHTML.replace(/<svg.*?<\/svg>|<img[^>]*>/g, "").slice(0, 1500));
  throw new Error(`the ${plugin} panel did not open: ${String(state)}`);
}

async function closePanel(plugin: string): Promise<void> {
  const icon = $(`.pstrip-icons button[aria-label='${plugin}']`);
  if ((await icon.getAttribute("aria-pressed")) === "true") await icon.click();
}

/** Creates a poll with the keyboard only: strip button, New poll, fields, Create poll. */
async function createPollByKeyboard(): Promise<void> {
  await closePanel("Poll");
  await $(".pstrip-icons button[aria-label='Poll']").waitForExist({ timeout: 30_000 });
  // Start from the message field, then walk the focus order to the strip.
  await composer().click();
  // The strip is a toolbar: one Tab stop, arrow keys move between its icons.
  await tabTo((f) => f.cls.includes("pstrip-icon"), "the Plugins strip", true);
  await pressUntil("ArrowRight", (f) => f.label === "Poll", "Poll in the Plugins strip");
  await browser.keys("Enter");
  await $(".pstrip-panel").waitForDisplayed({ timeout: 10_000 });
  await tabTo((f) => f.text === "New poll", "New poll");
  await browser.keys("Enter");
  await sheet().waitForDisplayed({ timeout: 10_000 });
  await tabTo((f) => f.tag === "input" && f.value === "", "the question field");
  await typeText(QUESTION);
  for (const option of ["Pizza", "Pasta"]) {
    await tabTo((f) => f.tag === "input" && f.value === "", `field for ${option}`);
    await typeText(option);
  }
  await tabTo((f) => f.text === "Create poll", "Create poll");
  await browser.keys("Enter");
  await waitText(() => $("body"), "Your poll · 0 votes");
  if (await sheet().isExisting()) await browser.keys("Escape");
}

/** Records every text the polite and assertive live regions announce. */
async function recordAnnouncements(): Promise<void> {
  await browser.execute(() => {
    const w = window as unknown as { __c14Said: string[] };
    w.__c14Said = [];
    for (const id of ["live-polite", "live-assertive"]) {
      const region = document.querySelector(`[data-testid='${id}']`);
      if (region === null) continue;
      new MutationObserver(() => {
        const text = region.textContent;
        if (text !== "") w.__c14Said.push(text);
      }).observe(region, { childList: true, subtree: true, characterData: true });
    }
  });
}

async function announcements(): Promise<string> {
  return browser.execute(() => (window as unknown as { __c14Said: string[] }).__c14Said.join("\n"));
}

async function openMusicDialog(): Promise<void> {
  const row = personRow(MUSIC_BOT);
  await row.waitForDisplayed({ timeout: 30_000 });
  // A contextmenu event like the menu key sends: WebKitWebDriver's right click also sends a click,
  // which pins the profile preview over the dialog a moment later.
  await browser.execute((el: HTMLElement) => {
    const box = el.getBoundingClientRect();
    el.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.left + 10, clientY: box.top + 5 }),
    );
  }, (await row.getElement()) as unknown as HTMLElement);
  const item = $("//*[@role='menuitem'][normalize-space()='Configure bot']");
  await item.waitForClickable({ timeout: 20_000 });
  await item.click();
  await dialog().waitForDisplayed({ timeout: 10_000 });
}

async function closeDialog(): Promise<void> {
  // The keyboard way out: Escape from wherever the focus is in the dialog.
  const where = describeFocus(await focus());
  expect(where).toContain("userpanel-modal__dialog");
  await browser.keys("Escape");
  await dialog().waitForExist({ reverse: true, timeout: 10_000 });
}

/** EVE fit's panel with its fit history opened. */
async function openFitHistory(): Promise<void> {
  await openPanel("EVE fit");
  const toggle = $(".pstrip-panel").$("button*=Fit history");
  await toggle.waitForClickable({ timeout: 20_000 });
  if ((await toggle.getText()).includes("▸")) await toggle.click();
  await waitText(() => $(".pstrip-panel"), "Tackle");
}

/** A local mumbot that runs only the music module (its HTTP API on 127.0.0.1). */
class MusicBot {
  private child: ChildProcess | undefined;
  private log = "";
  readonly key = randomBytes(16).toString("hex");
  readonly port = testenv.ports.v15 + 61;

  constructor(private readonly dir: string) {}

  private env(): NodeJS.ProcessEnv {
    return { ...process.env, MUMBOT_DATA: this.dir };
  }

  async start(): Promise<void> {
    const args = ["--name", MUSIC_BOT, "--port", String(testenv.ports.v15), "register"];
    for (let attempt = 1; ; attempt++) {
      try {
        await runOk(bin.bot, args, this.env());
        break;
      } catch (error) {
        if (attempt === 5) throw error;
        await new Promise((r) => setTimeout(r, 2_000));
      }
    }
    const media = join(this.dir, "media");
    mkdirSync(media, { recursive: true });
    const config = join(this.dir, "bot.toml");
    writeFileSync(
      config,
      `[server]\nhost = "127.0.0.1"\nport = ${String(testenv.ports.v15)}\nname = "${MUSIC_BOT}"\n` +
        `channel = "${CHANNEL}"\ndata_dir = "${this.dir}"\n` +
        `server_cert_sha256 = "${await certFingerprint("A")}"\n\n` +
        `[music]\nlisten = "127.0.0.1:${String(this.port)}"\nchannel = "${CHANNEL}"\nmedia_dir = "${media}"\n\n` +
        `[[music.keys]]\nkey = "${this.key}"\nlabel = "c14"\nchannels = ["${CHANNEL}"]\n`,
    );
    const child = spawn(bin.bot, ["music", "--config", config], { env: this.env() });
    child.stdout.on("data", (d: Buffer) => (this.log += d.toString()));
    child.stderr.on("data", (d: Buffer) => (this.log += d.toString()));
    this.child = child;
    const base = `http://127.0.0.1:${String(this.port)}`;
    await browser.waitUntil(
      async () =>
        (await fetch(`${base}/health`)
          .then((r) => r.json() as Promise<{ connected?: boolean }>)
          .catch(() => null))?.connected === true,
      { timeout: 60_000, timeoutMsg: `the music bot did not connect:\n${this.log}` },
    );
  }

  stop(): void {
    this.child?.kill("SIGTERM");
    this.child = undefined;
  }
}

describe("plugins: accessibility of the plugin UI (AC-6.29)", function () {
  this.bail(true);
  let fixtures: registry.FixtureRegistry;
  let carol: ScriptBot | undefined;
  let music: MusicBot;
  let dir: string;

  const env = (theme: "light" | "dark") => ({
    ...fixtures.env(),
    GTK_THEME: theme === "dark" ? "Adwaita:dark" : "Adwaita",
  });

  /** Restarts the app (same profile) in `theme`, reconnects and opens the channel chat. */
  async function restartIn(theme: "light" | "dark"): Promise<void> {
    carol?.stop();
    carol = undefined;
    await app.restart({ env: env(theme) });
    await reconnect();
    await joinChannel(CHANNEL);
    await selectChannel(CHANNEL);
    await browser.waitUntil(
      async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme,
      { timeoutMsg: `the app did not show the ${theme} theme` },
    );
  }

  async function startCarol(): Promise<ScriptBot> {
    const bot = new ScriptBot({ name: "Carol", port: testenv.ports.v15, timeout: 120 });
    await bot.connected();
    await bot.run(`join ${CHANNEL}`);
    carol = bot;
    return bot;
  }

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
    const eveFit = readdirSync(join(fixtures.workDir, "staged")).find((f) => f.startsWith("eve-fit-"));
    if (eveFit === undefined) throw new Error("eve-fit was not built (scripts/e2e-plugin-registry.sh)");
    await fixtures.publish(eveFit.replace(/\.mcplugin$/, ""));
    dir = mkdtempSync(join(tmpdir(), "mumcord-c14-"));
    music = new MusicBot(dir);
    await music.start();
    await app.restart({ env: env("light") });
    await completeWizard("Alice");
  });

  after(async () => {
    carol?.stop();
    music.stop();
    await fixtures.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  it("light: Settings → Plugins tabs, the install prompt and its progress bar have no violations", async () => {
    await developerMode();
    await checkPluginSettings("light-empty");
    await closeSettings();
    // Echolings keeps its pet per identity, which plugins only get once a server is focused.
    await addAndConnect();
    await joinChannel(CHANNEL);
    await selectChannel(CHANNEL);
    await openSettingsGroup("Plugins");
    await install("Poll", "light-poll");
    await install("Echolings");
    await install("Mumbot Music");
    await install("EVE fit");
    await checkPluginSettings("light");
    await closeSettings();
    expectNoFindings();
  });

  it("keyboard only: opens Poll from the strip, creates a poll and toggles the original text", async () => {
    await recordAnnouncements();
    await createPollByKeyboard();
    const item = cardItem("Poll");
    await item.waitForDisplayed({ timeout: 20_000 });
    // The card's buttons are in the Tab order after the message field.
    await composer().click();
    await tabTo((f) => f.text === "Show original text", "Show original text", true);
    await browser.keys("Enter");
    await expect(item).toHaveText(expect.stringContaining("[Poll]"), { wait: 5_000 });
    expect((await focus()).text).toBe("Hide original text");
    await browser.keys("Enter");
    await expect(item).not.toHaveText(expect.stringContaining("[Poll]"), { wait: 5_000 });
  });

  it("light: strip, Poll card with the original text, EVE fit history, Mumbot dialog, Echolings", async () => {
    const bot = await startCarol();
    await check("light-strip", [".pstrip"]);
    await check("light-poll-card", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Show original text").click();
    await check("light-poll-original", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Hide original text").click();

    // F46: a hidden vote line is not read out, a replaced fit is read as its card.
    await bot.run(`say --channel ${CHANNEL} "!poll 1"`);
    await bot.run(`say --channel ${CHANNEL} "${FIT.join("<br>")}"`);
    await cardItem("EVE fit").waitForDisplayed({ timeout: 20_000 });
    await browser.waitUntil(async () => (await announcements()).includes("EVE fit"), {
      timeout: 10_000,
      timeoutMsg: `the fit card was not announced; said:\n${await announcements()}`,
    });
    const said = await announcements();
    expect(said).not.toContain("!poll 1");
    expect(said).not.toContain(FIT[1]);
    expect(said).toContain("Carol");

    await openFitHistory();
    await check("light-evefit-history", [".pstrip"]);
    await closePanel("EVE fit");

    await openMusicDialog();
    await check("light-mumbot-setup", [".userpanel-modal__dialog"]);
    await setField(dialogField("Bot address (IP or domain, with port)"), `127.0.0.1:${String(music.port)}`);
    await setField(dialogField("API key"), music.key);
    await dialog()
      .$(".//label[normalize-space(.)='API key']/following::button[normalize-space(.)='Save'][1]")
      .click();
    await browser.waitUntil(async () => (await dialogField("API key").getValue()) === "", {
      timeoutMsg: "the API key was not stored",
    });
    await dialog().$(".//button[normalize-space(.)='Test connection']").click();
    await expect(dialog()).toHaveText(expect.stringContaining("Connection works"), { wait: 20_000 });
    const save = dialog().$(".//button[normalize-space(.)='Save'][not(ancestor::div[contains(@class,'pr-row')])]");
    await save.click();
    await expect(dialog()).toHaveText(expect.stringContaining("Nothing is playing"), { wait: 20_000 });
    await check("light-mumbot-player", [".userpanel-modal__dialog"]);
    await closeDialog();

    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await sheet().waitForDisplayed({ timeout: 10_000 });
    await waitText(sheet, "Choose your Echoling");
    await check("light-echolings-picker", [".pstrip-sheet"]);
    await setField(sheet().$("//div[contains(@class,'pr-field')][label[normalize-space(.)='Name']]//input"), "Pip");
    await sheet().$("button=Hatch").click();
    await waitText(sheet, "Pip");
    await sheet().$("button=Pat").click();
    await check("light-echolings", [".pstrip-sheet"]);
    await sheet().$("button.pstrip-expand").click();
    // Control for the reduced-motion check: with motion on, the own pet badge animates.
    await expect(personRow("Alice").$(".plugin-badge .pr-anim")).toExist({ wait: 10_000 });
    expectNoFindings();
  });

  it("F44: at a 150 % font size every clipped card offers Show more", async () => {
    await browser.execute(() => (document.documentElement.style.fontSize = "150%"));
    await browser.pause(1_000);
    const clipped = await browser.execute(() =>
      Array.from(document.querySelectorAll<HTMLElement>(".plugin-chat-card")).map((card) => {
        const body = card.querySelector<HTMLElement>(".plugin-chat-card__body");
        const overflows = body !== null && body.scrollHeight > body.clientHeight + 1;
        const more = Array.from(card.querySelectorAll("button")).some((b) => b.textContent === "Show more");
        return { label: card.getAttribute("aria-label") ?? "", overflows, more };
      }),
    );
    console.log(`[c14] cards at 150 %: ${JSON.stringify(clipped)}`);
    expect(clipped.length).toBeGreaterThan(0);
    const missing = clipped.filter((c) => c.overflows && !c.more).map((c) => c.label);
    expect(missing).toEqual([]);
    await check("light-cards-150", [".plugin-chat-card"]);
    await browser.execute(() => (document.documentElement.style.fontSize = ""));
    expectNoFindings();
  });

  it("dark: the same screens have no violations", async () => {
    await restartIn("dark");
    await startCarol();
    await createPollByKeyboard();
    await cardItem("Poll").waitForDisplayed({ timeout: 20_000 });
    await check("dark-strip", [".pstrip"]);
    await check("dark-poll-card", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Show original text").click();
    await check("dark-poll-original", ["section[aria-label='Card from Poll']"]);
    await cardItem("Poll").$("button=Hide original text").click();
    await carol?.run(`say --channel ${CHANNEL} "${FIT.join("<br>")}"`);
    await cardItem("EVE fit").waitForDisplayed({ timeout: 20_000 });
    await openFitHistory();
    await check("dark-evefit-history", [".pstrip"]);
    await closePanel("EVE fit");
    await openMusicDialog();
    await expect(dialog()).toHaveText(expect.stringContaining("Nothing is playing"), { wait: 20_000 });
    await check("dark-mumbot-player", [".userpanel-modal__dialog"]);
    await closeDialog();
    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await waitText(sheet, "Pip");
    await check("dark-echolings", [".pstrip-sheet"]);
    await sheet().$("button.pstrip-expand").click();
    await checkPluginSettings("dark");
    await install("Dice roller", "dark-dice");
    await closeSettings();
    expectNoFindings();
  });

  it("reduced motion (GTK animations off) freezes Echolings", async () => {
    const gtk = join(app.profile(), "config", "gtk-3.0");
    mkdirSync(gtk, { recursive: true });
    writeFileSync(join(gtk, "settings.ini"), "[Settings]\ngtk-enable-animations=false\n");
    await restartIn("light");
    expect(
      await browser.execute(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches),
    ).toBe(true);
    await openPanel("Echolings");
    await $(".pstrip-panel .pstrip-expand").click();
    await waitText(sheet, "Pip");
    await sheet().$("button=Pat").click();
    await browser.pause(1_000);
    const moving = await browser.execute(() => ({
      presets: document.querySelectorAll(".pr-anim").length,
      running: document
        .getAnimations()
        .filter((a) => {
          const target = (a.effect as KeyframeEffect | null)?.target;
          return target instanceof Element && target.closest(".plugin-badge, .pstrip, .pstrip-sheet") !== null;
        }).length,
    }));
    expect(moving).toEqual({ presets: 0, running: 0 });
    await sheet().$("button.pstrip-expand").click();
  });

  it("German: the permission texts are translated and the plugin settings have no violations", async () => {
    const de = JSON.parse(
      readFileSync(join(repoRoot, "ui", "src", "locales", "de", "pluginPermissions.json"), "utf8"),
    ) as { texts: { chatTriggers: string } };
    const installed = JSON.parse(
      readFileSync(join(repoRoot, "ui", "src", "locales", "de", "pluginInstalled.json"), "utf8"),
    ) as { details: { permissions: string } };
    await openSettingsGroup("General");
    await chooseLanguage("de", "German");
    await browser.waitUntil(async () => (await browser.execute(() => document.documentElement.lang)) === "de", {
      timeoutMsg: "the app did not switch to German",
    });
    await $("[role='tab'][id$='-tab-plugins']").click();
    const row = $(`//li[.//span[contains(@class,'plugins-installed-name')][normalize-space(.)='Poll']]`);
    await row.$(`.//button[normalize-space(.)='${installed.details.permissions}']`).click();
    await expect(row).toHaveText(expect.stringContaining(de.texts.chatTriggers), { wait: 10_000 });
    await check("de-settings-installed", [".overlay__dialog"]);
    await closeSettings();
    await check("de-strip", [".pstrip"]);
    expectNoFindings();
  });
});
