import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  selectChannel,
  sleep,
  waitMessage,
} from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

/** One second of a quiet 440 Hz tone as 16-bit mono PCM WAV. */
function toneWav(): Buffer {
  const rate = 8000;
  const samples = rate;
  const wav = Buffer.alloc(44 + samples * 2);
  wav.write("RIFF", 0);
  wav.writeUInt32LE(36 + samples * 2, 4);
  wav.write("WAVEfmt ", 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(rate, 24);
  wav.writeUInt32LE(rate * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write("data", 36);
  wav.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++) {
    wav.writeInt16LE(Math.round(Math.sin((2 * Math.PI * 440 * i) / rate) * 3000), 44 + i * 2);
  }
  return wav;
}

const MUSIC_PAGE = `<!doctype html><html><head>
  <meta property="og:type" content="music.song">
  <meta property="og:title" content="Midnight Signals">
  <meta property="og:description" content="Lumen Park · Single">
  </head><body>a song</body></html>`;

/** Local static site with an audio file, a page pretending to be one and a music page; records
 * every requested path. No internet in tests. */
function startSite(): Promise<{ server: Server; port: number; requests: string[] }> {
  const requests: string[] = [];
  const tone = toneWav();
  const server = createServer((req, res) => {
    const path = req.url ?? "/";
    requests.push(path);
    if (path.startsWith("/tone")) {
      res.writeHead(200, { "Content-Type": "audio/wav", "Content-Length": tone.length }).end(tone);
    } else if (path === "/broken.mp3") {
      res.writeHead(200, { "Content-Type": "text/html" }).end("<html>not audio</html>");
    } else if (path === "/song") {
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" }).end(MUSIC_PAGE);
    } else {
      res.writeHead(404).end();
    }
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, port: (server.address() as AddressInfo).port, requests });
    });
  });
}

async function say(html: string): Promise<void> {
  await bot(["say", "--channel", "Lobby", html], { name: "MusicBot", hold: 1 });
}

function audioCard(name: string) {
  return $(`article.audio-card[aria-label='Audio file: ${name}']`);
}

// Issues #260 (direct audio links) and #262 (music cards): nothing beyond a short probe or the
// page metadata loads before a click, and the privacy switch turns everything off.
describe("chat: audio and music embeds (#260, #262)", () => {
  let site: Awaited<ReturnType<typeof startSite>>;
  let base: string;

  before(async () => {
    site = await startSite();
    base = `http://127.0.0.1:${String(site.port)}`;
    await testenv.up("v15");
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  after(() => {
    site.server.close();
  });

  it("shows an audio player that fetches the file only on Play", async () => {
    const url = `${base}/tone.wav`;
    await say(`listen <a href="${url}">${url}</a>`);
    await waitMessage("listen");
    const card = audioCard("tone.wav");
    await card.waitForDisplayed({ timeout: 15_000 });
    await expect(card).toHaveText(expect.stringContaining("127.0.0.1"));
    await expect(card).toHaveText(expect.stringContaining("16 kB"));
    expect(site.requests.filter((p) => p === "/tone.wav")).toHaveLength(1);
    await expect(card.$("audio")).not.toExist();

    const play = card.$("button[aria-label='Play tone.wav']");
    await play.waitForEnabled();
    await play.click();
    await browser.waitUntil(() => site.requests.filter((p) => p === "/tone.wav").length === 2, {
      timeout: 10_000,
      timeoutMsg: "Play did not make the engine fetch the file",
    });
    // The web view plays a blob the engine handed over; it never contacts the site itself.
    await expect(card.$("audio")).toHaveAttribute("src", expect.stringMatching(/^blob:/));
  });

  it("shows the error state for a link that is not an audio file", async () => {
    const url = `${base}/broken.mp3`;
    await say(`broken <a href="${url}">${url}</a>`);
    await waitMessage("broken");
    const card = audioCard("broken.mp3");
    await card.waitForDisplayed({ timeout: 15_000 });
    await expect(card.$("[role='alert']")).toHaveText(
      expect.stringContaining("Can't play this file"),
    );
    await expect(card.$("button=Try again")).toBeDisplayed();
  });

  it("shows a music card from page metadata and a Spotify card that loads nothing before Play", async () => {
    const page = `${base}/song`;
    const spotify = "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC";
    await say(`song <a href="${page}">${page}</a>`);
    await say(`spotify <a href="${spotify}">${spotify}</a>`);
    const fallback = $("article.music-card[aria-label='Music link: Midnight Signals']");
    await fallback.waitForDisplayed({ timeout: 15_000 });
    await expect(fallback).toHaveText(expect.stringContaining("Lumen Park · Single"));
    await expect(fallback).toHaveText(expect.stringContaining("No player available"));
    await expect(fallback.$("button.music-card__play")).not.toExist();

    // The services cannot be reached in tests, so the card carries the fallback title.
    const card = $("article.music-card[aria-label='Spotify: Spotify link']");
    await card.waitForDisplayed({ timeout: 15_000 });
    await expect($$("iframe")).toBeElementsArrayOfSize(0);
    await card.$("button.music-card__play").click();
    await expect(card.$("iframe")).toHaveAttribute(
      "src",
      "https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC",
    );
    await card.$("button.music-card__play").click();
    await expect(card.$("iframe")).not.toExist();
  });

  it("with previews switched off shows plain links and fetches nothing", async () => {
    await openSettingsGroup("Privacy & Connections");
    const toggle = $(
      "//input[@role='switch'][@id=//label[.='Show link previews and embeds']/@for]",
    );
    await toggle.waitForExist();
    await toggle.click();
    await expect(toggle).not.toBeSelected();
    await browser.keys("Escape");

    const before = site.requests.length;
    const url = `${base}/tone-off.wav`;
    await say(`quiet <a href="${url}">${url}</a>`);
    await waitMessage("quiet");
    await sleep(2000);
    await expect($$("article.audio-card")).toBeElementsArrayOfSize(0);
    await expect($$("article.music-card")).toBeElementsArrayOfSize(0);
    expect(site.requests.length).toBe(before);
  });
});
