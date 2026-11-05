// Plugin sound specs (spec 09 §4.10, FR-9.43, FR-9.65 … FR-9.68): the `test-surface` fixture
// plugin installed from the third-party fixture repository with `audio.effects`, its panel
// buttons (user gestures for the plugin) and the measurement of its 660 Hz beep and the frame's
// 440 Hz tone in a recording.
import { $, browser, expect } from "@wdio/globals";
import * as app from "./app.ts";
import { addFirstServer, finishWizard } from "./chat-ui.ts";
import * as pluginenv from "./pluginenv.ts";
import { tonePower } from "./pwapps.ts";
import { openSettingsGroup } from "./settings-ui.ts";
import * as testenv from "./testenv.ts";
import { db, RATE } from "./voice-audio.ts";

export const ID = "test-surface";
export const NAME = "Test Surface";
/** The bundled `sounds/beep.wav`: 0.3 s of 660 Hz. */
export const BEEP_HZ = 660;
/** The tone the frame's "Play frame tone" button plays (web/probe.js). */
export const FRAME_TONE_HZ = 440;

/**
 * Starts the fixtures and an app connected to the v15 test server in Developer mode (the fixture
 * repository is plain http on localhost), adds the third-party repository and installs
 * `test-surface` with its optional `audio.effects` the way a confirmed install prompt does.
 * `audio` is the app's audio backend (see `app.restart`). Call `pluginenv.stopPluginFixtures()`
 * in `after`.
 */
export async function setUp(audio?: string): Promise<void> {
  const fixtures = await pluginenv.startPluginFixtures({ thirdParty: {} });
  await testenv.up("v15");
  await app.restart({ env: pluginenv.appEnv(), audio });
  await finishWizard("Alice");
  await addFirstServer(testenv.ports.v15);
  const join = $(".userpanel-readonly__join");
  await join.waitForClickable({ timeout: 15_000 });
  await join.click();
  await openSettingsGroup("Advanced & Developer");
  const developer = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
  await developer.waitForExist({ timeout: 10_000 });
  await developer.click();
  await expect(developer).toBeSelected();
  await closeSettings();
  await browser.waitUntil(
    async () => {
      try {
        await app.invoke("plugin_repo_add", { url: fixtures.thirdPartyUrl, expectedKey: null });
        return true;
      } catch {
        return false;
      }
    },
    { timeout: 10_000, timeoutMsg: "the fixture repository could not be added" },
  );
  await app.invoke("plugins_index", { language: "en" });
  await app.invoke("plugin_install_prepare", { id: ID });
  await app.invoke("plugin_install", {
    id: ID,
    version: null,
    optional: ["audio.effects"],
    visibility: "nobody",
    confirmed: true,
  });
  await browser.waitUntil(
    async () => {
      const list = await app.invoke<{ plugins: { id: string; status: { kind: string } }[] }>(
        "plugins_list",
      );
      return list.plugins.find((p) => p.id === ID)?.status.kind === "running";
    },
    { timeout: 30_000, timeoutMsg: `${ID} never started` },
  );
}

export async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/** Opens the plugin's panel in the Plugins strip (no-op when it is open). */
export async function openPanel(): Promise<void> {
  const icon = $(`button.pstrip-icon[aria-label='${NAME}']`);
  await icon.waitForClickable({ timeout: 20_000, timeoutMsg: `${NAME} has no strip icon` });
  if ((await icon.getAttribute("aria-pressed")) !== "true") await icon.click();
}
  await body.waitForExist({ timeout: 10_000, timeoutMsg: `${NAME}'s panel did not open` });
}

/** Clicks a button of the plugin's panel (a user gesture for the plugin). */
export async function panelButton(label: string): Promise<void> {
  await openPanel();
  const button = $(`//*[contains(@class,'pstrip-body')]//button[normalize-space()='${label}']`);
  await button.waitForClickable({ timeout: 10_000, timeoutMsg: `the panel has no "${label}"` });
  await button.click();
}

/** The panel's text lines (results and log), read from the DOM so scrolled-out lines count. */
export async function panelLines(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll(".pstrip-body .pr-text"), (el) => el.textContent.trim()),
  );
}

/** How often the panel shows `line` right now. */
export async function lineCount(line: string): Promise<number> {
  return (await panelLines()).filter((l) => l === line).length;
}

/** Waits until the panel shows `line` more than `before` times (the log repeats lines). */
export async function waitLine(line: string, before = 0, timeout = 15_000): Promise<void> {
  await openPanel();
  await browser.waitUntil(
    async () => (await panelLines()).filter((l) => l === line).length > before,
    { timeout, interval: 250, timeoutMsg: `the panel never showed "${line}"` },
  );
}

/** Clicks a panel button and waits for the plugin's log line `<action>: <result>`. */
export async function act(label: string, logLine: string): Promise<void> {
  await openPanel();
  const before = await lineCount(logLine);
  await panelButton(label);
  await waitLine(logLine, before);
}

/**
 * Loudest level (dBFS of an equivalent sine) of `hz` in `x`, over 30 ms windows: the peak of a
 * short sound, unaffected by other tones in the mix.
 */
export function toneLevel(x: Float32Array, hz: number): number {
  const w = Math.round(RATE * 0.03);
  let best = 0;
  for (let i = 0; i + w <= x.length; i += Math.round(w / 2)) {
    best = Math.max(best, tonePower(x.subarray(i, i + w), hz));
  }
  // tonePower is normalised to amplitude²; a sine of amplitude a has RMS a/√2.
  return db(Math.sqrt(best / 2));
}
