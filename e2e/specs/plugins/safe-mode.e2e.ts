// AC-6.8 … AC-6.10 (spec 06): a misbehaving plugin is stopped and restarted with back-off (5 s,
// 30 s) and turned off after the third failure with a label and a notification-centre entry,
// while voice keeps flowing; a WASI plugin is refused with a clear message; "Turn off all
// plugins" stays on across a restart; `--no-plugins` loads nothing and shows the banner.
// The time limits themselves are covered by the Rust tests in crates/plugins/tests/runtime.rs.
// The fixtures come from the run's third-party repository (scripts/e2e-plugin-registry.sh).
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app, registry, repoRoot, testenv } from "../../helpers/index.ts";
import { ScriptBot, addFirstServer, finishWizard } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { concat, readWav, writeWav } from "../../helpers/voice-audio.ts";

const ME = "SafeModeQA";
const LOOP = "Fixture Endless Loop";
const HOG = "Fixture Memory Hog";

const dir = mkdtempSync(join(tmpdir(), "mumcord-safe-mode-"));
const inWav = join(dir, "in.wav");
const outWav = join(dir, "out.wav");

interface Overview {
  noPlugins: boolean;
  allOff: boolean;
  plugins: { id: string; status: { kind: string } }[];
}

/**
 * Calls a backend command; a rejection comes back as its text instead of a thrown error. The
 * WebKit driver turns a rejected command into a WebDriver error even when the page catches it,
 * so the error is caught on this side.
 */
async function call(command: string, args: object = {}): Promise<{ ok: unknown; error?: string }> {
  try {
    return { ok: await app.invoke(command, args) };
  } catch (e) {
    return { ok: null, error: e instanceof Error ? e.message : String(e) };
  }
}

async function invokeOk(command: string, args: object = {}): Promise<unknown> {
  const result = await call(command, args);
  if (result.error !== undefined) throw new Error(`${command} failed: ${result.error}`);
  return result.ok;
}

async function overview(): Promise<Overview> {
  return (await invokeOk("plugins_list")) as Overview;
}

async function install(id: string): Promise<{ error?: string }> {
  await invokeOk("plugins_index", { language: "en" });
  return call("plugin_install", { id, version: null, optional: [], visibility: "nobody" });
}

async function openInstalled(): Promise<void> {
  await openSettingsGroup("Plugins");
  const tab = $(
    "//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'Installed')]",
  );
  await tab.waitForClickable({ timeout: 10_000 });
  await tab.click();
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** The state label of a plugin row in Settings → Plugins → Installed. */
function stateOf(name: string) {
  return $(
    `//li[.//span[contains(@class,'plugins-installed-name')][normalize-space(.)='${name}']]//span[contains(@class,'plugins-installed-state')]`,
  );
}

async function waitState(name: string, text: string, timeout: number): Promise<void> {
  await browser.waitUntil(
    async () => {
      const label = stateOf(name);
      return (await label.isExisting()) && (await label.getText()) === text;
    },
    { timeout, interval: 250, timeoutMsg: `${name} never showed "${text}"` },
  );
}

function banner() {
  return $(".plugins-banner");
}

function turnOffAll() {
  return $("//input[@role='switch'][@aria-labelledby][ancestor::div[contains(@class,'plugins-footer')]]");
}

describe("plugins: limits, back-off and safe mode (AC-6.8 … AC-6.10)", () => {
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
    // ~10 minutes of speech (the file backend plays silence after the end).
    writeWav(inWav, concat(...Array.from({ length: 100 }, () => speech)));
    fixtures = await registry.start();
    await testenv.up("v15");
    await app.restart({ audio: `file:${inWav}:${outWav}`, env: fixtures.env() });
    await finishWizard(ME);
    await addFirstServer(testenv.ports.v15);
    // A fresh connect is read-only (AC-1.35); join to talk.
    const joinButton = $(".userpanel-readonly__join");
    await joinButton.waitForClickable({ timeout: 15_000 });
    await joinButton.click();
    // The fixture repository is plain http on localhost, which only Developer mode accepts.
    await openSettingsGroup("Advanced & Developer");
    const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developer.waitForExist({ timeout: 10_000 });
    await developer.click();
    await expect(developer).toBeSelected();
    await closeSettings();
    await browser.waitUntil(
      async () =>
        (await call("plugin_repo_add", { url: fixtures.repoUrl, expectedKey: null })).error ===
        undefined,
      { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
    );
  });

  after(async () => {
    await fixtures.stop();
  });

  it("AC-6.8: an endless loop backs off 5 s, 30 s, then is turned off; voice keeps flowing", async function () {
    this.timeout(5 * 60_000);
    const ear = new ScriptBot({ name: "SafeEar", timeout: 120, port: testenv.ports.v15 });
    try {
      await ear.connected();
      await ear.run(`expect-audio --from ${ME} --min-rms -45 --timeout 25`, 40_000);

      const installed = Date.now();
      expect((await install("fx-endless-loop")).error).toBeUndefined();
      await openInstalled();
      // The first event after each start loops forever: stopped, then restarted after 5 s and 30 s.
      await waitState(LOOP, "Restarting after a crash", 15_000);
      await closeSettings();
      await ear.run(`expect-audio --from ${ME} --min-rms -45 --timeout 10`, 20_000);
      await openInstalled();
      await waitState(LOOP, "Turned off after repeated crashes", 90_000);
      const offAfter = (Date.now() - installed) / 1000;
      // Back-off 5 s + 30 s; anything clearly shorter means the timers were skipped.
      expect(offAfter).toBeGreaterThan(30);
      const row = (await overview()).plugins.find((p) => p.id === "fx-endless-loop");
      expect(row?.status.kind).toBe("autoOff");
      await closeSettings();

      await ear.run(`expect-audio --from ${ME} --min-rms -45 --timeout 10`, 20_000);
    } finally {
      ear.stop();
    }

    const bell = $("[data-testid='notification-bell']");
    await bell.waitForClickable({ timeout: 10_000 });
    await bell.click();
    const entry = $(
      `//section[@data-testid='notification-centre']//button[@data-testid='centre-entry'][contains(., '${LOOP} was turned off')]`,
    );
    await entry.waitForDisplayed({ timeout: 10_000 });
    expect(await entry.getText()).toContain("Turned off after repeated crashes");
    await browser.keys("Escape");
  });

  it("AC-6.9: running out of memory is handled like a crash; a WASI plugin is refused", async function () {
    this.timeout(3 * 60_000);
    expect((await install("fx-memory-hog")).error).toBeUndefined();
    await openInstalled();
    await waitState(HOG, "Restarting after a crash", 15_000);
    await invokeOk("plugin_set_enabled", { id: "fx-memory-hog", enabled: false });

    const refused = await install("fx-wasi-import");
    expect(refused.error).toContain("This plugin uses features Mumcord does not provide");
    const ids = (await overview()).plugins.map((p) => p.id);
    expect(ids).not.toContain("fx-wasi-import");
  });

  it("AC-6.10: Turn off all plugins stops everything and stays off after a restart", async function () {
    this.timeout(5 * 60_000);
    expect((await install("poll")).error).toBeUndefined();
    await browser.waitUntil(
      async () =>
        (await overview()).plugins.find((p) => p.id === "poll")?.status.kind === "running",
      { timeout: 20_000, timeoutMsg: "Poll never started" },
    );

    await openInstalled();
    await turnOffAll().click();
    await expect(banner()).toHaveText(expect.stringContaining("All plugins are turned off"), {
      wait: 10_000,
    });
    let state = await overview();
    expect(state.allOff).toBe(true);
    expect(state.plugins.every((p) => p.status.kind === "allOff")).toBe(true);

    await app.restart();
    await openInstalled();
    await expect(banner()).toHaveText(expect.stringContaining("All plugins are turned off"), {
      wait: 15_000,
    });
    state = await overview();
    expect(state.allOff).toBe(true);
    expect(state.plugins.find((p) => p.id === "poll")?.status.kind).toBe("allOff");
    expect(await turnOffAll().isSelected()).toBe(true);

    await turnOffAll().click();
    await browser.waitUntil(
      async () =>
        (await overview()).plugins.find((p) => p.id === "poll")?.status.kind === "running",
      { timeout: 20_000, timeoutMsg: "Poll did not start after turning plugins on again" },
    );
    await expect(banner()).not.toBeExisting();
  });

  it("AC-6.10: --no-plugins loads no plugin and shows the banner", async function () {
    this.timeout(3 * 60_000);
    await app.restart({ args: ["--no-plugins"] });
    await openInstalled();
    await expect(banner()).toHaveText(
      "Plugins are off for this session (started with --no-plugins).",
      { wait: 15_000 },
    );
    const state = await overview();
    expect(state.noPlugins).toBe(true);
    expect(state.plugins.some((p) => p.status.kind === "running")).toBe(false);
    expect(state.plugins.find((p) => p.id === "poll")?.status.kind).toBe("noPlugins");

    // Without the flag the plugin that was on runs again.
    await app.restart({ args: [] });
    await browser.waitUntil(
      async () =>
        (await overview()).plugins.find((p) => p.id === "poll")?.status.kind === "running",
      { timeout: 30_000, timeoutMsg: "Poll did not start again without --no-plugins" },
    );
  });
});
