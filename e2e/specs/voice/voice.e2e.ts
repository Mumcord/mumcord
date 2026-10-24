// AC-1.7 – AC-1.12 (spec §11): channel tree, talking indicator, join vs. view, voice activation,
// push-to-talk (X11), mute/deafen, noise suppression, per-person volume/mute persistence.
// Measurements are printed with a "[voice]" prefix for the QA report.
import { spawn, type ChildProcess } from "node:child_process";
import { cpSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, bot, botInBackground, repoRoot, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { run } from "../../helpers/run.ts";
import {
  activeLevel,
  activity,
  align,
  concat,
  db,
  levels,
  readWav,
  rms,
  silence,
  speechAndGap,
  wavSamples,
  writeWav,
} from "../../helpers/voice-audio.ts";
import {
  addAndConnect,
  channelName,
  completeWizard,
  personRow,
  readTree,
  reconnect,
  talkingOnTimes,
  watchTalking,
} from "../../helpers/voice-ui.ts";

const dir = join(repoRoot, "e2e", "artifacts", "voice");
const speechPath = join(repoRoot, "testenv", "audio", "speech.wav");
const noisyPath = join(repoRoot, "testenv", "audio", "speech_noisy.wav");
const speech = readWav(speechPath);
const noisy = readWav(noisyPath);
const f = (name: string) => join(dir, name);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const log = (msg: string) => {
  console.log(`[voice] ${msg}`);
};
const fmt = (n: number) => n.toFixed(1);

/** A `mumcord-bot script` on one connection; `do(line)` resolves when the bot printed `ok <line>`. */
class ScriptBot {
  readonly bg: BackgroundBot;
  private out = "";
  private seen = 0;
  readonly okTimes: { line: string; at: number }[] = [];

  constructor(name: string, cert?: string) {
    this.bg = botInBackground(["script"], { name, timeout: 20, ...(cert ? { cert } : {}) });
    this.bg.process.stdout?.on("data", (d: Buffer) => {
      const now = Date.now();
      this.out += d.toString();
      for (const l of d.toString().split("\n")) {
        if (l.startsWith("ok ")) this.okTimes.push({ line: l.slice(3), at: now });
      }
    });
    this.bg.done.catch((e: unknown) => {
      console.error(`[voice] script bot ${name} ended: ${String(e)}`);
    });
  }

  send(line: string): void {
    this.bg.send(line);
  }

  /** Sends `line` and waits for its `ok`; returns the stdout printed for it. */
  async do(line: string, timeoutMs = 30_000): Promise<string> {
    const before = this.out.length;
    this.send(line);
    return this.waitOk(line, timeoutMs, before);
  }

  async waitOk(line: string, timeoutMs = 30_000, from = 0): Promise<string> {
    const start = Date.now();
    for (;;) {
      const hit = this.okTimes.slice(this.seen).findIndex((o) => o.line === line);
      if (hit >= 0) {
        this.seen += hit + 1;
        return this.out.slice(from);
      }
      if (this.bg.process.exitCode !== null) throw new Error(`bot exited before "${line}"`);
      if (Date.now() - start > timeoutMs) throw new Error(`bot: no ok for "${line}"`);
      await sleep(10);
    }
  }

  okAt(line: string): number | undefined {
    return this.okTimes.filter((o) => o.line === line).pop()?.at;
  }
}

/** Plays `speech.wav` through the bot and returns the matching slice of the app's output WAV. */
async function playAndCapture(b: ScriptBot, out: string): Promise<Float32Array> {
  const s0 = wavSamples(out);
  await b.do(`play ${speechPath}`, 30_000);
  await sleep(1500);
  const s1 = wavSamples(out);
  return readWav(out).slice(s0, s1);
}

/** Copies the app's log files from the current profile into the artifacts folder. */
function copyLogs(label: string): void {
  const walk = (d: string): string[] =>
    readdirSync(d, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? walk(join(d, e.name)) : /log/i.test(join(d, e.name)) ? [join(d, e.name)] : [],
    );
  try {
    for (const file of walk(app.profile())) cpSync(file, f(`${label}-${file.split("/").pop() ?? "log"}`));
  } catch (e) {
    log(`copying logs failed: ${String(e)}`);
  }
}

function maxDb(x: Float32Array): number {
  return Math.max(-200, ...levels(x, 100));
}

describe("voice (AC-1.7 – AC-1.12)", () => {
  let main: ScriptBot;
  let bot2: BackgroundBot;
  let rootName = "Root";
  // The app shows the root under the server's registered name; the bot resolves the root by path.
  const botRoot = "/";

  before(async () => {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    writeWav(f("vad-in.wav"), concat(silence(8), speech, silence(14)));
    writeWav(f("mute-in.wav"), concat(silence(5), speech, speech, speech, speech, speech, speech));
    writeWav(f("noise-in.wav"), concat(silence(8), noisy, silence(60)));
    writeWav(f("volume-in.wav"), silence(2));
    writeWav(f("ptt-in.wav"), concat(noisy, noisy, noisy, noisy, noisy, noisy, noisy));
    await testenv.up("v15");
    // The server may still be starting (another spec may have just reset it).
    for (let i = 0; ; i++) {
      try {
        await bot(["connect"], { name: "Checker" });
        break;
      } catch (e) {
        if (i > 30) throw e;
        await sleep(1000);
      }
    }
    main = new ScriptBot("Bot");
    await main.do("join Lobby");
    bot2 = botInBackground(["join", "Raid"], { name: "Bot2", hold: 1200 });
  });

  after(() => {
    (main as ScriptBot | undefined)?.bg.stop();
    (bot2 as BackgroundBot | undefined)?.stop();
  });

  it("AC-1.7 shows the full tree with all bots in the right channels", async () => {
    await completeWizard("Tester");
    await addAndConnect();
    await browser.waitUntil(
      async () => {
        const tree = await readTree();
        const people = (c: string) => tree.find((t) => t.name === c)?.people ?? [];
        return people("Lobby").includes("Bot") && people("Raid").includes("Bot2");
      },
      { timeout: 15_000, timeoutMsg: "bots not shown in Lobby/Raid" },
    );
    const tree = await readTree();
    log(`tree ${JSON.stringify(tree)}`);
    const root = tree.find((c) => c.level === 1);
    rootName = root?.name ?? "";
    expect(root?.name).toBeTruthy();
    log(`root shown as "${rootName}" (bot uses "${botRoot}")`);
    for (const c of ["Lobby", "Raid", "AFK"]) {
      expect(tree.find((t) => t.name === c)?.level).toBe(2);
    }
    expect(root?.people).toContain("Tester");
    expect(tree.find((t) => t.name === "AFK")?.people ?? []).toHaveLength(0);
    await browser.saveScreenshot(f("tree.png"));
  });

  it("AC-1.8 single click shows Raid's chat without moving; double click joins", async () => {
    await channelName("Raid").click();
    await browser.waitUntil(
      async () => (await $("h2.chat-header__title").getText()).includes("Raid"),
      { timeoutMsg: "Raid chat not shown" },
    );
    await sleep(1500);
    const tree = await readTree();
    expect(tree.find((c) => c.level === 1)?.people).toContain("Tester");
    await bot(["expect", "user", "Tester", "--in", botRoot], { name: "Checker" });
    log("single click: chat header Raid, Tester still in Root (app tree + bot)");
    // The one-shot Checker bot's row leaving shifts the tree; double-click only on a settled layout.
    await browser.waitUntil(async () => !(await readTree()).some((c) => c.people.includes("Checker")), {
      timeout: 15_000,
      timeoutMsg: "Checker still shown",
    });
    await sleep(500);

    await browser.execute(() => {
      const w = window as unknown as { __dbl: number; __ev: string[] };
      w.__dbl = 0;
      w.__ev = [];
      document.addEventListener("dblclick", () => (w.__dbl += 1), true);
      for (const t of ["mousedown", "mouseup", "click", "dblclick"]) {
        document.addEventListener(
          t,
          (e) => {
            const el = e.target as HTMLElement;
            w.__ev.push(`${t}(d${String((e as MouseEvent).detail)} ${el.className || el.tagName} "${el.textContent.slice(0, 20)}")`);
          },
          true,
        );
      }
    });
    await channelName("Raid").doubleClick();
    await sleep(2000);
    const dbl = await browser.execute(() => (window as unknown as { __dbl: number }).__dbl);
    const ev = await browser.execute(() => (window as unknown as { __ev: string[] }).__ev);
    const joined = (await readTree()).find((c) => c.name === "Raid")?.people.includes("Tester");
    log(`double click (WebDriver): dblclick events ${String(dbl)}, Tester in Raid: ${String(joined)}; events ${ev.join(", ")}`);
    // No synthetic fallback: the real WebDriver double click must join on its own.
    await bot(["expect", "user", "Tester", "--in", "Raid"], { name: "Checker" });
    await browser.waitUntil(
      async () => (await readTree()).find((c) => c.name === "Raid")?.people.includes("Tester"),
      { timeoutMsg: "Tester not shown in Raid" },
    );
    log("double click: Bot sees Tester in Raid");
  });

  it("AC-1.7 talking indicator lights up within 200 ms", async () => {
    const hereC = (await readTree()).find((c) => c.people.includes("Tester"));
    const here = !hereC || hereC.level === 1 ? botRoot : hereC.name;
    await main.do(`join "${here}"`);
    await expect(personRow("Bot")).toBeExisting();
    await watchTalking();
    const lat: number[] = [];
    for (let i = 0; i < 5; i++) {
      const marker = `sleep 0.${String(i + 1)}`;
      main.send(marker);
      main.send(`play ${speechPath}`);
      await main.waitOk(marker);
      const t0 = main.okAt(marker) ?? 0;
      await main.waitOk(`play ${speechPath}`, 30_000);
      await sleep(1500);
      const on = (await talkingOnTimes("Bot")).find((t) => t >= t0 - 20);
      if (on === undefined) throw new Error("talking indicator never lit");
      lat.push(on - t0);
    }
    log(`talking latency ms ${JSON.stringify(lat)} max ${String(Math.max(...lat))}`);
    for (const l of lat) expect(l).toBeLessThanOrEqual(200);
  });

  it("AC-1.9 voice activation: speech is sent, silence is not", async () => {
    await main.do(`join "${botRoot}"`);
    // Back to the root, so the later restarts (which restore the last channel) land there.
    await browser.waitUntil(async () => !(await readTree()).some((c) => c.people.includes("Checker")), {
      timeout: 15_000,
      timeoutMsg: "Checker still shown",
    });
    await sleep(1000);
    await channelName(rootName).doubleClick();
    await main.do(`expect user Tester --in "${botRoot}"`);
    await app.restart({ audio: `file:${f("vad-in.wav")}:${f("vad-out.wav")}` });
    const rec = `record ${f("vad-rec.wav")} --seconds 32`;
    main.send(rec);
    await reconnect();
    await bot(["expect", "user", "Tester", "--in", botRoot], { name: "Checker" });
    await main.waitOk(rec, 40_000);
    const x = readWav(f("vad-rec.wav"));
    const act = activity(x, -50);
    const lv = levels(x);
    // Decoder fade-out tails (< -70 dBFS) right after the last packet count as silence.
    const zero = lv.filter((l, i) => (i < act.first || i > act.last) && l <= -70).length;
    const outside = lv.length - (act.last - act.first + 1);
    log(
      `VAD: active ${fmt(act.count / 10)} s (windows ${String(act.first)}..${String(act.last)} of ${String(act.total)}), ` +
        `level ${fmt(activeLevel(x, -50))} dB, speech input ${fmt(activeLevel(speech, -50))} dB, ` +
        `silent outside: ${String(zero)}/${String(outside)} windows digital silence, trailing ${fmt((act.total - act.last - 1) / 10)} s`,
    );
    expect(act.count).toBeGreaterThan(30);
    expect(act.count).toBeLessThan(95);
    expect(act.first).toBeGreaterThan(30);
    expect(act.total - act.last - 1).toBeGreaterThan(50);
    // Allow a window or two of timing slack at the speech edges (one decoder tail window can
    // straddle the boundary); everything else outside the speech must be digital silence.
    expect(zero).toBeGreaterThanOrEqual(Math.floor(outside * 0.98));
  });

  it("AC-1.10 self mute: Bot sees muted and gets no audio; deafen: nothing heard, Bot sees icon", async () => {
    const out = f("mute-out.wav");
    await app.restart({ audio: `file:${f("mute-in.wav")}:${out}` });
    await reconnect();
    await $("button[aria-label='Mute microphone']").click();
    await main.do("expect user Tester --muted true");
    await main.do(`record ${f("mute-rec.wav")} --seconds 8`, 20_000);
    const muted = readWav(f("mute-rec.wav"));
    log(`mute: Bot sees muted; recorded max ${fmt(maxDb(muted))} dB while input speech plays`);
    expect(maxDb(muted)).toBeLessThan(-90);

    await $("button[aria-label='Mute microphone']").click();
    const hit = await main.do("expect-audio --from Tester --min-rms -40 --timeout 10");
    log(`unmute: audio back: ${hit.split("\n")[0] ?? ""}`);

    await $("button[aria-label='Deafen']").click();
    await expect($("button[aria-label='Deafen']")).toHaveAttribute("aria-pressed", "true");
    await main.do("expect user Tester --deaf true");
    await sleep(1000);
    const deaf = await playAndCapture(main, out);
    log(`deafen: Bot sees deaf; app output during Bot speech max ${fmt(maxDb(deaf))} dB (${fmt(deaf.length / 48000)} s)`);
    expect(maxDb(deaf)).toBeLessThan(-70);

    await $("button[aria-label='Deafen']").click();
    await main.do("expect user Tester --deaf false");
    await sleep(1000);
    const heard = await playAndCapture(main, out);
    log(`undeafen control: app output level ${fmt(activeLevel(heard, -50))} dB`);
    expect(activeLevel(heard, -50)).toBeGreaterThan(-45);
  });

  it("AC-1.11 noise suppression reduces keyboard/fan noise (VAD)", async () => {
    const out = f("noise-out.wav");
    await app.restart({ audio: `file:${f("noise-in.wav")}:${out}` });
    const rec = `record ${f("noise-rec.wav")} --seconds 32`;
    main.send(rec);
    await reconnect();
    await main.waitOk(rec, 40_000);
    const x = readWav(f("noise-rec.wav"));
    const lag = align(x, speech);
    const inM = speechAndGap(noisy, speech, 0);
    const outM = speechAndGap(x, speech, lag);
    // The clip starts abruptly after digital silence; the first ~150 ms (AGC at full gain, the
    // suppressor's model warming up) is not representative of a continuously noisy microphone.
    const inS = speechAndGap(noisy, speech, 0, 15);
    const outS = speechAndGap(x, speech, lag, 15);
    log(
      `noise (VAD, onset 150 ms skipped): input SNR ${fmt(inS.speechDb - inS.gapDb)} dB (gap ${fmt(inS.gapDb)}), ` +
        `sent SNR ${fmt(outS.speechDb - outS.gapDb)} dB (gap ${fmt(outS.gapDb)}); improvement ${fmt(outS.speechDb - outS.gapDb - (inS.speechDb - inS.gapDb))} dB, ` +
        `noise floor in pauses ${fmt(outS.gapDb - inS.gapDb)} dB; artifact (listen) ${f("noise-rec.wav")}`,
    );
    log(
      `noise (VAD): input speech ${fmt(inM.speechDb)} / gap ${fmt(inM.gapDb)} dB (SNR ${fmt(inM.speechDb - inM.gapDb)}); ` +
        `sent speech ${fmt(outM.speechDb)} / gap ${fmt(outM.gapDb)} dB (SNR ${fmt(outM.speechDb - outM.gapDb)}); ` +
        `gap windows transmitted ${String(outM.gapSentWindows)}/${String(outM.gapWindows)}; lag ${String(lag * 10)} ms`,
    );
    expect(outM.speechDb).toBeGreaterThan(-45);
    expect(outS.speechDb - outS.gapDb - (inS.speechDb - inS.gapDb)).toBeGreaterThan(12);
    expect(outS.gapDb - inS.gapDb).toBeLessThan(-12);
  });

  it("AC-1.12 volume 50 % and local mute survive rename + reconnect", async () => {
    // Own launch and output file, so this test does not depend on AC-1.11 having run (or passed).
    // The microphone plays silence (after the end of the file, too).
    const out = f("volume-out.wav");
    await app.restart({ audio: `file:${f("volume-in.wav")}:${out}` });
    await reconnect();
    await main.do(`expect user Tester --in "${botRoot}"`);
    const l100 = activeLevel(await playAndCapture(main, out), -55);

    await personRow("Bot").click();
    const slider = $(".profile-card__volume input[type='range']");
    await slider.waitForDisplayed();
    expect(await slider.getValue()).toBe("100");
    await slider.click();
    await browser.keys("Home");
    for (let i = 0; i < 10; i++) await browser.keys("ArrowRight");
    await expect($(".profile-card__volume output")).toHaveText("50 %");
    await browser.keys("Escape");
    await sleep(800);
    const l50 = activeLevel(await playAndCapture(main, out), -65);

    // The bot's `rename` keeps its old voice channel, so the renamed identity reconnects as a new
    // bot process with the same certificate (same person to the app, new name and session).
    // (A `rename` immediately followed by stopping the process races the app's view.)
    main.bg.stop();
    await browser.waitUntil(async () => !(await personRow("Bot").isExisting()), { timeout: 15_000 });
    main = new ScriptBot("BotRenamed", "Bot");
    await main.do(`join "${botRoot}"`);
    await personRow("BotRenamed").waitForExist({ timeout: 15_000 });
    await sleep(500);
    const l50r = activeLevel(await playAndCapture(main, out), -65);
    await personRow("BotRenamed").click();
    await expect($(".profile-card__volume output")).toHaveText("50 %");
    const mute = $("//label[contains(@class,'profile-card__mute')]/input");
    await mute.click();
    await expect(mute).toBeSelected();
    await browser.keys("Escape");
    await sleep(800);
    const mutedCap = await playAndCapture(main, out);
    const mutedMax = maxDb(mutedCap);
    writeWav(f("mute12-first.wav"), mutedCap);
    const leak = activity(mutedCap, -60);
    log(`local mute leak windows ${String(leak.count)} (first ${String(leak.first)} last ${String(leak.last)} of ${String(leak.total)})`);
    log(
      `local mute capture: max ${fmt(mutedMax)} dB, active windows ${String(activity(mutedCap, -60).count)}; ` +
        `tree ${JSON.stringify(await readTree())}`,
    );

    main.bg.stop();
    await browser.waitUntil(async () => !(await personRow("BotRenamed").isExisting()), { timeout: 15_000 });
    main = new ScriptBot("Bot");
    await main.do(`join "${botRoot}"`);
    await personRow("Bot").waitForExist({ timeout: 15_000 });
    await sleep(500);
    const mutedCapR = await playAndCapture(main, out);
    const mutedMaxR = maxDb(mutedCapR);
    writeWav(f("mute12-after-reconnect.wav"), mutedCapR);
    const leakR = activity(mutedCapR, -60);
    log(
      `local mute after 2nd reconnect: max ${fmt(mutedMaxR)} dB, active windows ${String(leakR.count)} ` +
        `(first ${String(leakR.first)} last ${String(leakR.last)} of ${String(leakR.total)}, 100 ms); tree ${JSON.stringify(await readTree())}`,
    );
    await personRow("Bot").click();
    await expect($(".profile-card__volume output")).toHaveText("50 %");
    await expect($("//label[contains(@class,'profile-card__mute')]/input")).toBeSelected();
    // Control: unmuting makes Bot audible again, still at 50 %.
    await $("//label[contains(@class,'profile-card__mute')]/input").click();
    await browser.keys("Escape");
    await sleep(800);
    const l50u = activeLevel(await playAndCapture(main, out), -65);
    log(`after unmute (2nd reconnect): ${fmt(l50u)} dB (${fmt(l50u - l100)})`);
    log(
      `volume: 100% ${fmt(l100)} dB, 50% ${fmt(l50)} dB (${fmt(l50 - l100)}), after rename+reconnect ${fmt(l50r)} dB (${fmt(l50r - l100)}); ` +
        `local mute max ${fmt(mutedMax)} dB, after 2nd rename+reconnect max ${fmt(mutedMaxR)} dB`,
    );
    expect(l50 - l100).toBeLessThan(-4);
    expect(Math.abs(l50r - l50)).toBeLessThan(1.5);
    expect(mutedMax).toBeLessThan(-70);
    expect(mutedMaxR).toBeLessThan(-70);
    expect(Math.abs(l50u - l50)).toBeLessThan(1.5);
  });

  it("AC-1.9 push-to-talk (X11): sent only while the key is held, other window focused", async () => {
    await $("button[aria-label='Settings']").click();
    await $("button[role='tab']=Voice").click();
    await $("//label[contains(., 'Push-to-talk')]").click();
    const keyButton = $("button=Set key");
    await keyButton.click();
    await browser.keys("F9");
    await browser.waitUntil(async () => (await $("kbd.settings-kbd").getText()) !== "none", {
      timeoutMsg: "push-to-talk key not captured",
    });
    log(`ptt key: ${await $("kbd.settings-kbd").getText()}`);
    const panel = await $("[role='tabpanel']").getText();
    log(`voice settings notes: ${panel.split("\n").filter((l) => /not available|cannot|could not/i.test(l)).join(" | ") || "none"}`);
    await browser.keys("Escape");

    await app.restart({ audio: `file:${f("ptt-in.wav")}:${f("ptt-out.wav")}` });
    await reconnect();
    await main.do(`expect user Tester --in "${botRoot}"`);

    const xdo = async (...args: string[]) => (await run("xdotool", args)).stdout.trim();
    const before = (await xdo("search", "--onlyvisible", "--name", ".")).split("\n");
    const mumcordWin = (await xdo("search", "--onlyvisible", "--name", "^Mumcord$")).split("\n")[0] ?? "";
    const viewer: ChildProcess = spawn("gcr-viewer-gtk4", [join(repoRoot, "testenv/certs/A/cert.pem")], {
      stdio: "ignore",
    });
    const focusLog: string[] = [];
    try {
      let win = "";
      for (let i = 0; i < 50 && !win; i++) {
        await sleep(200);
        const now = (await xdo("search", "--onlyvisible", "--name", ".")).split("\n");
        win = now.find((w) => w && !before.includes(w)) ?? "";
      }
      if (!win) throw new Error("helper window did not appear");

      const rec = `record ${f("ptt-rec.wav")} --seconds 24`;
      // A (control): key held 3–10 s with Mumcord focused.
      if (mumcordWin) await xdo("windowfocus", "--sync", mumcordWin);
      main.send(rec);
      await sleep(3000);
      focusLog.push(`A focus "${await xdo("getwindowfocus", "getwindowname")}" (Mumcord ${mumcordWin})`);
      await xdo("keydown", "F9");
      await sleep(7000);
      await xdo("keyup", "F9");
      // B: key held 14–21 s while the helper window has focus.
      await xdo("windowfocus", "--sync", win);
      await sleep(4000);
      focusLog.push(`B focus "${await xdo("getwindowfocus", "getwindowname")}" (helper ${win})`);
      await xdo("keydown", "F9");
      await sleep(7000);
      await xdo("keyup", "F9");
      await main.waitOk(rec, 30_000);
    } finally {
      viewer.kill();
      copyLogs("ptt");
    }
    log(`PTT focus: ${focusLog.join("; ")}`);
    const x = readWav(f("ptt-rec.wav"));
    const sec = (a: number, b: number) => x.slice(a * 48000, b * 48000);
    const actA = activity(sec(2.5, 11), -60);
    const actB = activity(sec(13.5, 22), -60);
    const quiet = [sec(0, 2.8), sec(10.6, 13.8), sec(21.6, 24)].map(maxDb);
    log(
      `PTT: Mumcord focused (control): active ${fmt(actA.count / 10)} s of 7 s held; other window focused: active ${fmt(actB.count / 10)} s of 7 s held; ` +
        `max level while released ${quiet.map(fmt).join(" / ")} dB`,
    );
    for (const name of ["A", "B"]) {
      const held = name === "A" ? sec(3.2, 10.2) : sec(14.2, 21.2);
      if (maxDb(held) < -60) continue;
      const lag = align(held, speech);
      const inM = speechAndGap(noisy, speech, 0);
      const outM = speechAndGap(held, speech, lag);
      log(
        `noise (PTT ${name}, continuous): input speech ${fmt(inM.speechDb)} / gap ${fmt(inM.gapDb)} dB (SNR ${fmt(inM.speechDb - inM.gapDb)}), ` +
          `sent speech ${fmt(outM.speechDb)} / gap ${fmt(outM.gapDb)} dB (SNR ${fmt(outM.speechDb - outM.gapDb)}), ` +
          `gap windows transmitted ${String(outM.gapSentWindows)}/${String(outM.gapWindows)}; total rms ${fmt(db(rms(held)))} dB`,
      );
    }
    for (const q of quiet) expect(q).toBeLessThan(-70);
    expect(actA.count).toBeGreaterThan(60);
    expect(actB.count).toBeGreaterThan(60);
  });
});
