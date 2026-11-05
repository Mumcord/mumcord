// Spec 09 (plugin platform 1.1), AC-9.4: a plugin page in a busy loop does not take Mumcord down.
//
// `test-surface` runs in the stage while Alice talks (a looped speech file as her microphone) and
// a bot records the channel. "Busy loop" makes the stage page spin forever. On Linux the page
// shares the app window's web process, so the window stops answering (spec 09 §13.1); the Rust
// UI watchdog (FR-9.42) notices after 5 s, turns the plugin's web content off and restarts the
// window's web process. The test checks:
//   - the app process survives the freeze and the watchdog's restart;
//   - voice never stopped: the bot's recording has no gap during the freeze;
//   - the plugin's web content stays turned off (also after a relaunch), Diagnostics says why, and
//     "Turn web content on" brings the stage back.
//
// Deviation and manual parts:
//   - WebKitWebDriver ends its session when the watchdog restarts the window's web process ("page
//     crash or hang"), and a new session launches a new app. So the UI checks after the freeze
//     run in a relaunched app on the same profile; the freeze count (in memory) and the talk
//     indicator during the freeze cannot be observed from the test and are manual (AC-9.40).
//   - On Linux the window itself freezes, so the frame cannot show "<plugin> stopped responding"
//     inside a live window. That path (frame heartbeat, FR-9.42) applies where frames run in
//     their own process (WebView2) and is part of the manual checklist (AC-9.40), as is the macOS
//     child web view.
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { botInBackground, type BackgroundBot } from "../../helpers/bot.ts";
import * as app from "../../helpers/app.ts";
import { pluginenv, repoRoot } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import * as surface from "../../helpers/test-surface.ts";
import { concat, levels, readWav, writeWav } from "../../helpers/voice-audio.ts";

/** The longest silence (in 100 ms windows) the recording may have between Alice's speech. */
const MAX_GAP_WINDOWS = 20;
const RECORD_SECONDS = 40;

const stageFrame = () => $(".plugin-stage iframe.plugin-webview__frame");

/** Whether the app window answers (`ok`), hangs (`frozen`) or the WebDriver session is gone. */
async function windowState(): Promise<"ok" | "frozen" | "lost"> {
  try {
    const ok = await app.withTimeout(
      browser.execute(() => document.querySelector(".pstrip-icon") !== null),
      2_000,
      "window probe",
    );
    return ok ? "ok" : "frozen";
  } catch (e) {
    return /invalid session|page crash or hang|session deleted/i.test(String(e)) ? "lost" : "frozen";
  }
}

/** Whether process `pid` still runs (zombies count as gone). */
function running(pid: number): boolean {
  try {
    return !/^\d+ \(.*\) Z/.test(readFileSync(`/proc/${String(pid)}/stat`, "utf8"));
  } catch {
    return false;
  }
}

/** The longest run of windows below -50 dBFS between the first and the last active window. */
function longestGap(path: string): { gap: number; active: number } {
  const lv = levels(readWav(path));
  const first = lv.findIndex((l) => l >= -50);
  let last = lv.length - 1;
  while (last >= 0 && (lv[last] ?? -100) < -50) last--;
  let gap = 0;
  let run = 0;
  let active = 0;
  for (let i = Math.max(first, 0); i <= last; i++) {
    if ((lv[i] ?? -100) >= -50) {
      active++;
      run = 0;
    } else {
      run++;
      gap = Math.max(gap, run);
    }
  }
  return { gap, active };
}

describe("plugins 1.1: a busy plugin page does not freeze Mumcord (AC-9.4)", function () {
  this.timeout(20 * 60_000);
  const dir = mkdtempSync(join(tmpdir(), "mumcord-watchdog-"));
  const recording = join(dir, "rec.wav");
  let recorder: BackgroundBot | undefined;

  before(async function () {
    this.timeout(15 * 60_000);
    // Four minutes of speech, so the microphone still talks after the setup.
    const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
    writeWav(join(dir, "in.wav"), concat(...Array<Float32Array>(40).fill(speech)));
    await surface.setUp({ audio: `file:${join(dir, "in.wav")}:${join(dir, "out.wav")}` });
    await surface.panelButton("Start stage");
    await stageFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the stage frame did not open" });
    await surface.waitLine(/^ready: stage$/);
  });

  after(async () => {
    recorder?.stop();
    await pluginenv.stopPluginFixtures();
    rmSync(dir, { recursive: true, force: true });
  });

  it("AC-9.4: the busy page freezes the window, the watchdog recovers it and the app survives", async function () {
    this.timeout(3 * 60_000);
    recorder = botInBackground(["record", recording, "--seconds", String(RECORD_SECONDS)], {
      name: "Recorder",
      timeout: 30,
    });
    // Let the recording start before the freeze.
    await browser.pause(5_000);
    const pid = app.appPid();
    await surface.panelButton("Busy loop");
    let froze = false;
    let state: "ok" | "frozen" | "lost" = "ok";
    await browser.waitUntil(
      async () => {
        state = await windowState();
        if (state !== "ok") froze = true;
        // Done once the window answered again after the freeze, or the restart ended the session.
        return state === "lost" || (froze && state === "ok");
      },
      { timeout: 60_000, interval: 500, timeoutMsg: "the window never froze or never recovered" },
    );
    console.log(`[watchdog] after the freeze the WebDriver session is ${state}`);
    // The watchdog restarted the web process, not the app.
    await browser.pause(3_000);
    expect(running(pid)).toBe(true);
    expect(app.appPid()).toBe(pid);
  });

  it("AC-9.4: voice kept going during the freeze", async function () {
    this.timeout(3 * 60_000);
    await recorder?.done;
    const { gap, active } = longestGap(recording);
    console.log(`[watchdog] recording: ${String(active)} active windows, longest gap ${String(gap)}`);
    expect(active).toBeGreaterThan(100);
    expect(gap).toBeLessThanOrEqual(MAX_GAP_WINDOWS);
  });

  it("AC-9.4: web content stays off, Diagnostics says why, and turning it on again works", async function () {
    this.timeout(5 * 60_000);
    // A new session means a new app on the same profile (see the header).
    await app.restart();
    await $(".pstrip-icon").waitForExist({ timeout: 60_000, timeoutMsg: "the app did not come back" });
    // The stage needs a joined channel, as in the setup; the relaunched app may rejoin by itself.
    const join = $(".userpanel-readonly__join");
    if (await join.waitForClickable({ timeout: 30_000 }).catch(() => false)) await join.click();
    const diagnostics = (await surface.invokeOk("plugin_diagnostics", { id: surface.ID })) as {
      webDisabled: boolean;
    };
    expect(diagnostics.webDisabled).toBe(true);
    // While it is off, the plugin cannot show its page.
    await surface.panelButton("Start stage");
    await surface.waitLine(/^start-stage: (?!ok$)/);

    await openSettingsGroup("Plugins");
    await $(
      "//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'Installed')]",
    ).click();
    const row = $(
      `//li[.//span[contains(@class,'plugins-installed-name')][normalize-space()='${surface.NAME}']]`,
    );
    await row.$(".//button[@aria-controls]").click();
    await row.$(".//button[normalize-space()='Diagnostics']").click();
    await expect(
      row.$(
        ".//p[normalize-space()='Its web content is turned off because Mumcord stopped responding while it was open.']",
      ),
    ).toBeDisplayed({ wait: 10_000 });
    await row.$(".//button[normalize-space()='Turn web content on']").click();
    await row
      .$(".//p[starts-with(normalize-space(),'Its web content is turned off')]")
      .waitForExist({ reverse: true, timeout: 10_000 });
    await browser.keys("Escape");
    await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });

    await surface.panelButton("Start stage");
    await stageFrame().waitForExist({ timeout: 15_000, timeoutMsg: "the stage did not open again" });
  });
});
