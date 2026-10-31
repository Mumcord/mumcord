// Issue #67 (FR-3.70, FR-3.71, AC-3.58): a whisper target created from the person menu, started
// from the user panel: only the target hears the user, a bot in the user's own channel does not,
// the banner says to whom, and Stop returns the voice to the channel.
import { mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, bot, botInBackground, repoRoot, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { concat, db, readWav, rms, silence, writeWav } from "../../helpers/voice-audio.ts";
import {
  addAndConnect,
  channelName,
  completeWizard,
  doubleClickRow,
  personRow,
} from "../../helpers/voice-ui.ts";

const dir = join(repoRoot, "e2e", "artifacts", "whisper");
const speech = readWav(join(repoRoot, "testenv", "audio", "speech.wav"));
const f = (name: string) => join(dir, name);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A `mumbot script` on one connection; `do(line)` resolves when the bot printed `ok <line>`. */
class ScriptBot {
  readonly bg: BackgroundBot;
  private out = "";

  constructor(name: string) {
    this.bg = botInBackground(["script"], { name, timeout: 20 });
    this.bg.process.stdout?.on("data", (d: Buffer) => {
      this.out += d.toString();
    });
    this.bg.done.catch((e: unknown) => {
      if (this.bg.process.signalCode !== null) return;
      console.error(`[whisper] script bot ${name} ended: ${String(e)}`);
    });
  }

  async do(line: string, timeoutMs = 30_000): Promise<string> {
    const from = this.out.length;
    this.bg.send(line);
    const start = Date.now();
    for (;;) {
      const done = this.out.indexOf(`ok ${line}`, from);
      if (done >= 0) return this.out.slice(from, done);
      if (this.bg.process.exitCode !== null) throw new Error(`bot exited before "${line}"`);
      if (Date.now() - start > timeoutMs) throw new Error(`bot: no ok for "${line}"`);
      await sleep(20);
    }
  }
}

const banner = () => $("//div[contains(concat(' ', @class, ' '), ' whisper-banner ')]");

describe("voice: whisper and shout (#67)", () => {
  let hearer: ScriptBot;
  let bystander: ScriptBot;

  before(async () => {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    // The microphone talks the whole time, so who hears it shows where the voice goes.
    writeWav(f("in.wav"), concat(silence(2), ...Array.from({ length: 30 }, () => speech)));
    await testenv.up("v15");
    for (let i = 0; ; i++) {
      try {
        await bot(["connect"], { name: "WhisperCheck" });
        break;
      } catch (e) {
        if (i > 30) throw e;
        await sleep(1000);
      }
    }
    hearer = new ScriptBot("Hearer");
    await hearer.do("join Raid");
    bystander = new ScriptBot("Bystander");
    await bystander.do("join Lobby");
    await app.restart({ freshProfile: true, audio: `file:${f("in.wav")}:${f("out.wav")}` });
    await completeWizard("Tester");
    await addAndConnect();
    await doubleClickRow(channelName("Lobby"));
    await bystander.do("expect user Tester --in Lobby");
  });

  after(() => {
    (hearer as ScriptBot | undefined)?.bg.stop();
    (bystander as ScriptBot | undefined)?.bg.stop();
  });

  it("creates a whisper target with “Whisper to…” in the person menu", async () => {
    await personRow("Hearer").click({ button: "right" });
    await $("//button[@role='menuitem'][contains(., 'Whisper to…')]").click();
    const name = $("//input[@placeholder='Name']");
    await name.waitForDisplayed({ timeout: 10_000 });
    await expect(name).toHaveValue("Hearer");
    await expect($("//input[@type='radio'][@id=//label[.='People']/@for]")).toBeSelected();
    await $("//form[contains(@class,'whisper-setup__editor')]//button[.='Save']").click();
    await $("//p[@role='status'][.='Saved.']").waitForDisplayed();
    await browser.keys("Escape");
    await name.waitForDisplayed({ reverse: true });
  });

  it("AC-3.58 only the target hears the whisper, and the banner says to whom", async () => {
    // Normal talking first: the bystander in Lobby hears the user.
    await bystander.do("expect-audio --from Tester --min-rms -40 --timeout 15");

    await $("button[aria-label='Whisper or shout']").click();
    await $("//div[@role='menu']//button[@role='menuitem'][contains(., 'Hearer')]").click();
    await banner().waitForDisplayed();
    await expect(banner()).toHaveText(expect.stringContaining("Whispering to Hearer"));
    // The user panel's status strip says it, too.
    await expect($("button[aria-label='Whispering to Hearer · Stop']")).toBeDisplayed();

    await hearer.do("expect-audio --from Tester --min-rms -40 --timeout 15");
    // Let the last channel packets drain, then the bystander must hear nothing.
    await sleep(500);
    await bystander.do(`record ${f("bystander.wav")} --from Tester --seconds 4`, 20_000);
    const heard = readWav(f("bystander.wav"));
    console.log(`[whisper] bystander level while whispering: ${db(rms(heard)).toFixed(1)} dB`);
    expect(db(rms(heard))).toBeLessThan(-70);
  });

  it("Stop ends the whisper and the channel hears the user again", async () => {
    await banner().$(".//button[.='Stop']").click();
    await banner().waitForDisplayed({ reverse: true });
    await bystander.do("expect-audio --from Tester --min-rms -40 --timeout 15");
  });
});
