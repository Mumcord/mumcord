import { existsSync, statSync, unlinkSync } from "node:fs";
import { join } from "node:path";
import { $, browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import {
  button,
  dumpAppLogs,
  ensureArtifacts,
  labelled,
  longSpeechWav,
  readWavPcm,
  waitForText,
  windowDb,
  wizardArtifacts,
  wizardStep,
} from "../../helpers/wizard-util.ts";

// AC-1.3: Wizard step 2 → speak → level meter moves; "Test" plays back the own voice.
// The file audio backend feeds speech as microphone input and records the speakers to a WAV.
describe("AC-1.3 wizard microphone step", () => {
  const inWav = join(wizardArtifacts, "ac13-in.wav");
  const outWav = join(wizardArtifacts, "ac13-out.wav");

  afterEach(function () {
    if (this.currentTest?.state === "failed") dumpAppLogs(app.profile());
  });

  before(async () => {
    ensureArtifacts();
    longSpeechWav(inWav, 20); // 2 minutes of speech, so the mic still talks when we reach step 2
    if (existsSync(outWav)) unlinkSync(outWav);
    await app.restart({ audio: `file:${inWav}:${outWav}` });
  });

  it("level meter moves with speech and Test plays the recording back", async () => {
    await waitForText(wizardStep(1), 30_000);
    await (await labelled("Name")).setValue("Tester");
    await button("Next").click();
    await waitForText(wizardStep(2), 120_000);

    const meter = $('[role="meter"][aria-label="Microphone level"]');
    await meter.waitForExist();
    expect(Number(await meter.getAttribute("aria-valuenow"))).toBe(0);

    // Speaker output written so far (bytes after the 44-byte header); playback must come after.
    await browser.pause(1500);
    const before = Math.max(0, (statSync(outWav).size - 44) / 2);

    await button("Test").click();
    await waitForText("Speak now…", 5_000);
    const levels: number[] = [];
    const t0 = Date.now();
    while (Date.now() - t0 < 4000) {
      levels.push(Number(await meter.getAttribute("aria-valuenow")));
      await browser.pause(100);
    }
    const distinct = new Set(levels).size;
    console.log(
      `AC-1.3 meter samples: n=${String(levels.length)} max=${String(Math.max(...levels))} distinct=${String(distinct)} [${levels.join(",")}]`,
    );
    expect(Math.max(...levels)).toBeGreaterThan(10);
    expect(distinct).toBeGreaterThan(3);

    await waitForText("Playing back your voice…", 10_000);
    await waitForText("Press Test, say something", 20_000);
    await browser.pause(1500); // file backend refreshes the WAV header once per second

    const pcm = readWavPcm(outWav);
    const pre = windowDb(pcm.subarray(0, before));
    const post = windowDb(pcm.subarray(before));
    const voiced = post.filter((db) => db > -40).length;
    console.log(
      `AC-1.3 output: before-test max ${Math.max(-120, ...pre).toFixed(1)} dBFS; after-test max ${Math.max(...post).toFixed(1)} dBFS, ${String(voiced)} x 100 ms windows > -40 dBFS`,
    );
    expect(Math.max(-120, ...pre)).toBeLessThan(-50);
    expect(voiced).toBeGreaterThan(10); // > 1 s of audible playback
  });
});
