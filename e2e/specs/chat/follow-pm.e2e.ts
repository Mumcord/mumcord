import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, artifactsDir, bot, repoRoot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  composer,
  finishWizard,
  joinChannel,
  OutputWav,
  ScriptBot,
  selectChannel,
  sendText,
  silentWav,
  sleep,
  unreadBadge,
  USER,
  waitMessage,
} from "../../helpers/chat-ui.ts";

const speech = join(repoRoot, "testenv", "audio", "speech.wav");

function dmItem() {
  return $("//section[contains(@class,'direct-messages')]//button[contains(.,'RaidBot')]");
}

function privateHeader() {
  return $("//h2[contains(@class,'chat-header__title') and contains(.,'RaidBot')]");
}

function followButton() {
  return $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
}

// AC-1.15 (follow: text arrives, ear icon, no audio), AC-1.16 (deny-listen part) and AC-1.18
// (private message: chat + sound + reply). Speakers are recorded to a WAV via the file backend.
describe("chat: follow and private messages (AC-1.15, AC-1.16, AC-1.18)", () => {
  const out = new OutputWav(join(artifactsDir, "chat-follow-out.wav"));
  let raidBot: ScriptBot;

  before(async () => {
    await testenv.up("v15");
    await bot(["allow-listen", "AFK"], { superuser: true }).catch(() => undefined);
    await app.restart({ audio: `file:${silentWav("chat-silence.wav", 600)}:${out.path}` });
    await finishWizard();
    await addFirstServer(64738);
    await joinChannel("Lobby");
    raidBot = new ScriptBot({ name: "RaidBot" });
    await raidBot.connected();
    await raidBot.run("join Raid");
  });

  after(async () => {
    raidBot.stop();
    await bot(["allow-listen", "AFK"], { superuser: true }).catch(() => undefined);
  });

  it("AC-1.15 Follow Raid from Lobby: message arrives with an unread badge", async () => {
    await selectChannel("Raid");
    await followButton().click();
    await expect(followButton()).toHaveAttribute("aria-pressed", "true");
    await bot(["expect", "user", USER, "--in", "Lobby", "--listening", "Raid"], { name: "EarBot" });
    await selectChannel("Lobby");
    await raidBot.run('say --channel Raid "hello from raid"');
    await browser.waitUntil(async () => (await unreadBadge("Raid")) === "1", {
      timeoutMsg: "no unread badge 1 on Raid",
    });
    await selectChannel("Raid");
    await waitMessage("hello from raid");
  });

  it("AC-1.15 no audio from the followed channel reaches the speakers", async () => {
    await selectChannel("Lobby");
    // Warm-up: the output recording restarts (file truncated) when the first remote voice
    // arrives, so measure only a second clip.
    await raidBot.run(`play ${speech}`, 20_000);
    await sleep(1500);
    const before = out.length();
    await raidBot.run(`play ${speech}`, 20_000);
    await sleep(1500);
    const after = out.length();
    expect(after - before).toBeGreaterThan(48_000 * 6);
    const raidDb = out.peakDb(before, after);
    console.warn(`AC-1.15 peak while Raid talks: ${raidDb.toFixed(1)} dBFS`);
    expect(raidDb).toBeLessThan(-60);

    // Positive control: the same voice in the user's own channel is audible in the recording.
    await raidBot.run("join Lobby");
    await sleep(1500); // join sound
    const before2 = out.length();
    await raidBot.run(`play ${speech}`, 20_000);
    await sleep(1000);
    const after2 = out.length();
    expect(after2 - before2).toBeGreaterThan(48_000 * 6);
    const lobbyDb = out.peakDb(before2, after2);
    console.warn(`AC-1.15 control peak in own channel: ${lobbyDb.toFixed(1)} dBFS`);
    expect(lobbyDb).toBeGreaterThan(-45);
    await raidBot.run("join Raid");
  });

  it("AC-1.16 Follow on a channel with Listen denied shows a clear error", async () => {
    await bot(["deny-listen", "AFK"], { superuser: true });
    await selectChannel("AFK");
    await expect(followButton()).toHaveAttribute("aria-pressed", "false");
    await followButton().click();
    const error = $("p.follow-error[role='alert']");
    await error.waitForDisplayed({ timeout: 10_000 });
    const text = await error.getText();
    console.warn(`AC-1.16 deny-listen error: ${text}`);
    expect(text).toContain("doesn't allow you to follow");
    await expect(followButton()).toHaveAttribute("aria-pressed", "false");
    await bot(["expect", "user", USER, "--not-listening", "AFK"], { name: "EarBot" });
  });

  it("AC-1.18 private message opens a private chat with a sound; reply reaches the bot", async () => {
    await selectChannel("Lobby");
    await sleep(1000);
    const before = out.length();
    const baseline = out.peakDb(Math.max(0, before - 48_000), before);
    await raidBot.run(`pm ${USER} "secret hello"`);
    await dmItem().waitForDisplayed({ timeout: 10_000 });
    await sleep(1500);
    const after = out.length();
    expect(after).toBeGreaterThan(before);
    const soundDb = out.peakDb(before, after);
    console.warn(`AC-1.18 baseline ${baseline.toFixed(1)} dBFS, after PM ${soundDb.toFixed(1)} dBFS`);
    expect(soundDb).toBeGreaterThan(-50);
    expect(soundDb).toBeGreaterThan(baseline + 20);
    // Open it with the keyboard (focus + Enter); the mouse path is checked separately below.
    await browser.execute(() => {
      document.querySelector<HTMLButtonElement>(".direct-messages__item")?.focus();
    });
    await browser.keys("Enter");
    await privateHeader().waitForDisplayed();
    await waitMessage("secret hello");
    await sendText("private pong");
    await raidBot.run(`expect message --from ${USER} --private true --contains "private pong"`);
  });

  it("AC-1.18 a mouse click on the Direct messages entry opens the chat (tree focused)", async () => {
    await selectChannel("Lobby"); // leaves focus in the channel tree, as after any channel click
    await dmItem().click();
    const opened = await privateHeader()
      .waitForDisplayed({ timeout: 5_000 })
      .then(() => true)
      .catch(() => false);
    if (!opened) {
      // Diagnose: same click with focus moved out of the tree first (the tree's keyboard hint
      // is only shown while the tree has focus, so blurring it shifts the list below).
      await composer().click();
      await dmItem().click();
      const afterBlur = await privateHeader()
        .waitForDisplayed({ timeout: 5_000 })
        .then(() => true)
        .catch(() => false);
      throw new Error(
        `mouse click on the DM entry did not open the chat while the tree had focus (works after blurring the tree: ${String(afterBlur)})`,
      );
    }
  });
});
