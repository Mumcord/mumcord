// Spec 09 (plugin platform 1.1), AC-9.4: a plugin page in a busy loop does not take Mumcord down.
//
// `test-surface` runs in the stage while Alice talks (a looped speech file as her microphone) and
// a bot records the channel. "Busy loop" makes the stage page spin forever. On Linux the page
// shares the app window's web process, so the window stops answering (spec 09 §13.1); the Rust
// UI watchdog (FR-9.42) notices after 5 s, turns the plugin's web content off and restarts the
// window's web process. The test checks:
//   - the window answers again and the frame is gone;
//   - voice never stopped: the bot's recording has no gap during the freeze, and Alice's own talk
//     indicator lights up again;
//   - Diagnostics shows the freeze and that the web content is off; "Turn web content on" brings
//     the stage back.
//
// Deviation and manual parts:
//   - On Linux the window itself freezes, so the frame cannot show "<plugin> stopped responding"
//     inside a live window, and the watchdog needs its 5 s plus the window's reload: the test
//     allows 30 s from the click until the window answers. The in-window "stopped responding"
//     path (frame heartbeat, FR-9.42) applies where frames run in their own process (WebView2) and
//     is part of the manual checklist (AC-9.40), as is the macOS child web view.
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { botInBackground, type BackgroundBot } from "../../helpers/bot.ts";
import { pluginenv, repoRoot } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import * as surface from "../../helpers/test-surface.ts";
import { concat, levels, readWav, writeWav } from "../../helpers/voice-audio.ts";
import { talkingOnTimes, watchTalking } from "../../helpers/voice-ui.ts";

/** The longest silence (in 100 ms windows) the recording may have between Alice's speech. */
const MAX_GAP_WINDOWS = 20;
const RECORD_SECONDS = 40;

const stageFrame = () => $(".plugin-stage iframe.plugin-webview__frame");

/** Whether the app window runs scripts again; false while it is frozen or reloading. */
async function answers(): Promise<boolean> {
  try {
    return await browser.execute(
      () => document.readyState === "complete" && document.querySelector(".pstrip-icon") !== null,
    );
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
  let clickedAt = 0;
  let answeredAt = 0;

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

  it("AC-9.4: the busy page freezes the window only until the watchdog restarts it", async function () {
    this.timeout(3 * 60_000);
    recorder = botInBackground(["record", recording, "--seconds", String(RECORD_SECONDS)], {
      name: "Recorder",
      timeout: 30,
    });
    // Let the recording start before the freeze.
    await browser.pause(5_000);
    clickedAt = Date.now();
    await surface.panelButton("Busy loop");
    await browser.waitUntil(answers, {
      timeout: 60_000,
      interval: 500,
      timeoutMsg: "the app window never answered again",
    });
    answeredAt = Date.now();
    console.log(`[watchdog] window answered again ${String(answeredAt - clickedAt)} ms after the click`);
    expect(answeredAt - clickedAt).toBeLessThan(30_000);
    await expect(stageFrame()).not.toBeExisting();
  });

  it("AC-9.4: voice kept going during the freeze and the talk indicator works", async function () {
    this.timeout(3 * 60_000);
    await recorder?.done;
    const { gap, active } = longestGap(recording);
    console.log(`[watchdog] recording: ${String(active)} active windows, longest gap ${String(gap)}`);
    expect(active).toBeGreaterThan(100);
    expect(gap).toBeLessThanOrEqual(MAX_GAP_WINDOWS);
    await watchTalking();
    await browser.waitUntil(async () => (await talkingOnTimes("Alice")).length > 0, {
      timeout: 20_000,
      timeoutMsg: "Alice's talk indicator never lit after the restart",
    });
  });

  it("AC-9.4: Diagnostics shows the freeze and turning web content on again works", async () => {
    const diagnostics = (await surface.invokeOk("plugin_diagnostics", { id: surface.ID })) as {
      uiFreezes: number;
      webDisabled: boolean;
    };
    expect(diagnostics.uiFreezes).toBe(1);
    expect(diagnostics.webDisabled).toBe(true);
    // While it is off, the plugin cannot show its page.
    await surface.panelButton("Start stage");
    await surface.waitLine(/^start-stage: (?!ok$)/);

    await openSettingsGroup("Plugins");
    await $("//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'Installed')]").click();
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
    await expect(
      row.$(".//*[contains(@class,'plugins-installed-diagnostics-pair')][contains(.,'Times Mumcord froze while its web content was open')]"),
    ).toHaveText(expect.stringContaining("1"));
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
