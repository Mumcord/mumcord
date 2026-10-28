// Positional audio (C9): AC-1.31 (voices placed left/right and quieter with distance, measured
// on the app's stereo output), AC-1.32 (a Mumble game plugin delivers positions) and AC-1.55
// (switch in quick settings and settings stay in sync; "3D" icon for the own entry and for a Bot).
// The fake game is fake-link.py (Mumble Link shared memory); the game plugin is the in-repo test
// plugin, which looks for a program named after MUMCORD_TEST_PLUGIN_GAME.
import { spawn, type ChildProcess } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import {
  app,
  artifactsDir,
  bin,
  botInBackground,
  repoRoot,
  testenv,
  type BackgroundBot,
} from "../../helpers/index.ts";
import { hasClass } from "../../helpers/selectors.ts";
import { readWav, rms, silence, splitStereo, wavSamples, writeWav } from "../../helpers/voice-audio.ts";
import { addAndConnect, completeWizard, joinIfReadOnly, personRow, readTree } from "../../helpers/voice-ui.ts";

const dir = join(artifactsDir, "positional");
const f = (name: string) => join(dir, name);
const speechPath = join(repoRoot, "testenv", "audio", "speech.wav");
const fakeLink = join(repoRoot, "e2e", "specs", "positional", "fake-link.py");
const GAME_CONTEXT = "mumcord-e2e";
const PLUGIN_GAME = "mumcord-fake-game";
const PLUGIN_NAME = "Mumcord Test Game";

interface PositionalStatus {
  enabled: boolean;
  source: string | null;
  game: string | null;
  senders: number[];
}

const status = () => app.invoke<PositionalStatus>("positional_status");

function log(message: string): void {
  console.log(`[positional] ${message}`);
}

/** Starts a fake game process; resolves once it runs (fake-link.py prints "ready"). */
function startProcess(command: string, args: string[], ready?: string): Promise<ChildProcess> {
  const child = spawn(command, args, { stdio: ["ignore", "pipe", "pipe"] });
  if (ready === undefined) return Promise.resolve(child);
  return new Promise((resolvePromise, reject) => {
    let out = "";
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
      if (out.includes(ready)) resolvePromise(child);
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      reject(new Error(`${command} exited (${String(code)}) before it was ready`));
    });
  });
}

/** Waits until a `script` bot printed `ok <line>`. */
function waitOk(b: BackgroundBot, line: string, timeout = 30_000): Promise<void> {
  return new Promise((resolvePromise, reject) => {
    let out = "";
    const timer = setTimeout(() => {
      reject(new Error(`bot never finished "${line}"`));
    }, timeout);
    b.process.stdout?.on("data", (d: Buffer) => {
      out += d.toString();
      if (out.includes(`ok ${line}`)) {
        clearTimeout(timer);
        resolvePromise();
      }
    });
  });
}

async function openQuickSettings(): Promise<ReturnType<typeof $>> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  // Positional audio sits in the collapsible Beta group, which remembers whether it was open.
  const beta = $("//button[@role='menuitem'][@aria-expanded][contains(., 'Beta')]");
  if ((await beta.getAttribute("aria-expanded")) === "false") await beta.click();
  const row = $("//button[@role='menuitemcheckbox'][contains(., 'Positional audio')]");
  await row.waitForDisplayed();
  return row;
}

async function openAudioSettings(): Promise<void> {
  await (await openQuickSettings()).waitForDisplayed();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab' and .='Audio']").click();
}

/** The settings switch whose label is `label`. */
function settingsSwitch(label: string) {
  return $(`//div[${hasClass('settings-switch')}][.//label[.='${label}']]//input[@role='switch']`);
}

async function closeDialog(): Promise<void> {
  await browser.keys("Escape");
  await $(".overlay__dialog").waitForExist({ reverse: true, timeout: 5000 });
}

function badge(name: string) {
  return personRow(name).$(".positional-badge");
}

describe("positional audio (AC-1.31, AC-1.32, AC-1.55)", () => {
  let here = "/";
  let game: ChildProcess | undefined;
  let pluginGame: ChildProcess | undefined;
  const bots: BackgroundBot[] = [];
  const out = f("out.wav");

  before(async () => {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    writeWav(f("in.wav"), silence(1));
    // The plugin goes into Mumcord's own plugin folder; the test plugin reads its game's name
    // from the environment the app inherits.
    const plugins = join(app.profile(), "data", "app.mumcord.client", "plugins");
    mkdirSync(plugins, { recursive: true });
    copyFileSync(bin.testPlugin, join(plugins, "libtest_plugin.so"));
    process.env.MUMCORD_TEST_PLUGIN_GAME = PLUGIN_GAME;

    await testenv.reset("v15");
    await app.restart({ audio: `file-stereo:${f("in.wav")}:${out}` });
    await completeWizard("Tester");
    await addAndConnect();
    await joinIfReadOnly();
    const hereC = (await readTree()).find((c) => c.people.includes("Tester"));
    here = !hereC || hereC.level === 1 ? "/" : hereC.name;
  });

  after(() => {
    game?.kill();
    pluginGame?.kill();
    for (const b of bots) b.stop();
    delete process.env.MUMCORD_TEST_PLUGIN_GAME;
    rmSync(`/dev/shm/MumbleLink.${String(process.getuid?.() ?? 0)}`, { force: true });
  });

  it("AC-1.55 the quick settings switch and the settings switch follow each other", async () => {
    let row = await openQuickSettings();
    expect(await row.getAttribute("aria-checked")).toBe("false");
    await row.click();
    await expect(row).toHaveAttribute("aria-checked", "true");
    await browser.keys("Escape");

    await openAudioSettings();
    const toggle = settingsSwitch("Positional audio");
    await expect(toggle).toBeSelected();
    await toggle.click();
    await expect(toggle).not.toBeSelected();
    await closeDialog();
    expect((await status()).enabled).toBe(false);

    row = await openQuickSettings();
    await expect(row).toHaveAttribute("aria-checked", "false");
    await row.click();
    await expect(row).toHaveAttribute("aria-checked", "true");
    await browser.keys("Escape");
    expect((await status()).enabled).toBe(true);
  });

  it("AC-1.55 the own entry shows the 3D icon while a Mumble Link game delivers positions", async () => {
    game = await startProcess("python3", ["-I", fakeLink, "E2E Game", GAME_CONTEXT], "ready");
    await browser.waitUntil(async () => (await status()).source === "Mumble Link", {
      timeout: 15_000,
      timeoutMsg: "Mumble Link game not detected",
    });
    await badge("Tester").waitForExist({ timeout: 10_000, timeoutMsg: "no 3D icon on the own entry" });
    expect(await badge("Tester").getAttribute("aria-label")).toBeTruthy();
  });

  it("AC-1.31 Bots on the left and the right are heard from their side, the farther one quieter", async () => {
    const heard = new Map<string, [number, number]>();
    for (const [name, position] of [
      ["Left", "-3,0,0"],
      ["Right", "6,0,0"],
    ] as const) {
      const b = botInBackground(["script"], { name, timeout: 60 });
      bots.push(b);
      const joined = waitOk(b, "sleep 1");
      b.send(`join "${here}"`);
      b.send(`game-context ${GAME_CONTEXT}`);
      b.send("sleep 1");
      await joined;
      const s0 = wavSamples(out);
      const play = `play ${speechPath} --position ${position}`;
      const played = waitOk(b, play);
      b.send(play);
      // AC-1.55: a Bot sending positional voice packets gets the 3D icon.
      if (name === "Left") await badge(name).waitForExist({ timeout: 10_000, timeoutMsg: "no 3D icon on Left" });
      await played;
      await browser.pause(1500);
      const [l, r] = splitStereo(readWav(out).slice(s0 - (s0 % 2), wavSamples(out)));
      heard.set(name, [rms(l), rms(r)]);
      log(`${name} at ${position}: L ${rms(l).toFixed(4)} R ${rms(r).toFixed(4)}`);
    }
    const [ll, lr] = heard.get("Left") ?? [0, 0];
    const [rl, rr] = heard.get("Right") ?? [0, 0];
    expect(ll).toBeGreaterThan(0.001);
    expect(ll).toBeGreaterThan(2 * lr);
    expect(rr).toBeGreaterThan(2 * rl);
    expect(ll + lr).toBeGreaterThan(1.5 * (rl + rr));
  });

  it("AC-1.55 the 3D icon disappears a few seconds after positional data stops", async () => {
    // Left stopped talking during the previous test and is still connected.
    await badge("Left").waitForExist({ reverse: true, timeout: 10_000, timeoutMsg: "3D icon stays on Left" });
    for (const b of bots.splice(0)) b.stop();

    game?.kill("SIGTERM");
    game = undefined;
    await badge("Tester").waitForExist({
      reverse: true,
      timeout: 15_000,
      timeoutMsg: "3D icon stays on the own entry after the game stopped",
    });
    expect((await status()).source).toBeNull();
  });

  it("AC-1.32 an enabled game plugin delivers positions while its game runs", async () => {
    pluginGame = await startProcess("bash", ["-c", `exec -a ${PLUGIN_GAME} sleep 600`]);
    await openAudioSettings();
    const toggle = settingsSwitch(PLUGIN_NAME);
    await toggle.waitForExist({ timeout: 15_000, timeoutMsg: "test plugin not listed" });
    await expect(toggle).not.toBeSelected();
    await toggle.click();
    await expect(toggle).toBeSelected();

    const plugin = $(`//div[${hasClass('settings-switch')}][.//label[.='${PLUGIN_NAME}']]`);
    await browser.waitUntil(async () => (await plugin.getText()).includes("Delivering positions"), {
      timeout: 15_000,
      timeoutMsg: "plugin never shown as delivering positions",
    });
    await $(`//p[@role='status'][contains(., 'Positions from: ${PLUGIN_NAME}')]`).waitForExist();
    await closeDialog();
    // Positions applied: the own voice packets carry them, so the own entry shows 3D.
    await badge("Tester").waitForExist({ timeout: 10_000, timeoutMsg: "no 3D icon from the plugin's positions" });
    expect((await status()).source).toBe(PLUGIN_NAME);
  });
});
