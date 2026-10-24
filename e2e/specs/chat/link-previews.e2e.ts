import { createServer, type Server } from "node:http";
import { readFileSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  finishWizard,
  installOpenerStub,
  joinChannel,
  reconnectSaved,
  selectChannel,
  sleep,
  waitMessage,
} from "../../helpers/chat-ui.ts";

/** 1×1 red PNG as the page's preview image. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
  "base64",
);

/** Two-frame 1×1 GIF (red, then blue), looping: an animation that must stay a GIF. */
const ANIMATED_GIF = Buffer.from([
  // Header, 1×1 screen, global colour table with red and blue
  0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0x00, 0x00,
  0x00, 0x00, 0xff,
  // NETSCAPE2.0 loop extension
  0x21, 0xff, 0x0b, 0x4e, 0x45, 0x54, 0x53, 0x43, 0x41, 0x50, 0x45, 0x32, 0x2e, 0x30, 0x03, 0x01,
  0x00, 0x00, 0x00,
  // Frame 1: colour 0
  0x21, 0xf9, 0x04, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01,
  0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
  // Frame 2: colour 1
  0x21, 0xf9, 0x04, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01,
  0x00, 0x00, 0x02, 0x02, 0x4c, 0x01, 0x00,
  0x3b,
]);

function page(title: string): string {
  return `<!doctype html><html><head><title>fallback</title>
    <meta property="og:title" content="${title}">
    <meta property="og:description" content="A page served by the E2E test.">
    <meta property="og:site_name" content="Mumcord Test Site">
    <meta property="og:image" content="/cover.png"></head><body>hello</body></html>`;
}

/** Local static site with OpenGraph tags; records every requested path. No internet in tests. */
function startSite(): Promise<{ server: Server; port: number; requests: string[] }> {
  const requests: string[] = [];
  const server = createServer((req, res) => {
    const path = req.url ?? "/";
    requests.push(path);
    if (path === "/cover.png" || path.startsWith("/photo")) {
      res.writeHead(200, { "Content-Type": "image/png" }).end(PNG);
    } else if (path.startsWith("/anim")) {
      res.writeHead(200, { "Content-Type": "image/gif" }).end(ANIMATED_GIF);
    } else if (path.startsWith("/page")) {
      res
        .writeHead(200, { "Content-Type": "text/html; charset=utf-8" })
        .end(page(`Preview ${path.slice(1)}`));
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

function card(title: string) {
  return $(`//article[contains(@class,'link-card')][.//p[@class='link-card__title' and .='${title}']]`);
}

function inlineImage(name: string) {
  return $(`figure.inline-image[aria-label='Image: ${name}']`);
}

async function say(html: string): Promise<void> {
  await bot(["say", "--channel", "Lobby", html], { name: "LinkBot", hold: 1 });
}

async function openChatSettings(): Promise<void> {
  const button = $("button[aria-label='Settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='tab' and .='Chat & history']").click();
}

// AC-1.39 (link preview card, switch off → plain link and no request), AC-1.40 (YouTube and
// Twitch player cards that load nothing before a click) and AC-1.45 (external images and image
// links inline, nothing loaded with the switch off).
describe("chat: link previews, player cards and inline images (AC-1.39, AC-1.40, AC-1.45)", () => {
  let site: Awaited<ReturnType<typeof startSite>>;
  let opener: { log: string; binDir: string };

  before(async () => {
    site = await startSite();
    await testenv.up("v15");
    opener = installOpenerStub();
    process.env.PATH = `${opener.binDir}:${process.env.PATH ?? ""}`;
    await app.restart();
    await finishWizard();
    await addFirstServer(64738);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  after(() => {
    site.server.close();
  });

  it("AC-1.39 shows a preview card with title, image and Open in browser", async () => {
    const url = `http://127.0.0.1:${String(site.port)}/page-one`;
    await say(`look <a href="${url}">${url}</a>`);
    await waitMessage("look");
    const preview = card("Preview page-one");
    await preview.waitForDisplayed({ timeout: 15_000 });
    await expect(preview).toHaveText(expect.stringContaining("Mumcord Test Site"));
    await expect(preview).toHaveText(expect.stringContaining("A page served by the E2E test."));
    const image = preview.$("img.link-card__image");
    await expect(image).toHaveAttribute("src", expect.stringMatching(/^data:image\/(png|jpeg);base64,/));
    expect(site.requests).toEqual(["/page-one", "/cover.png"]);

    await preview.$("button.link-card__open").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(url), {
      timeout: 10_000,
      timeoutMsg: "Open in browser did not hand the link to the system browser",
    });
  });

  it("AC-1.40 shows YouTube and Twitch player cards that load nothing until play", async () => {
    const youtube = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const twitch = "https://www.twitch.tv/mumcordtest";
    await say(`<a href="${youtube}">${youtube}</a>`);
    await say(`<a href="${twitch}">${twitch}</a>`);
    // Fetching the providers is blocked in tests (no internet), so the cards carry the fallback title.
    const yt = card("YouTube video");
    const tw = card("Twitch video");
    await yt.waitForDisplayed({ timeout: 15_000 });
    await tw.waitForDisplayed({ timeout: 15_000 });
    await expect(tw).toHaveText(expect.stringContaining("mumcordtest"));
    await expect($$("iframe")).toBeElementsArrayOfSize(0);

    // Linux serves the UI from tauri://, where the players cannot run: play opens the browser.
    await yt.$("button.link-card__play").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(youtube), {
      timeout: 10_000,
      timeoutMsg: "play did not open the video in the system browser",
    });
    await tw.$("button.link-card__open").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(twitch), {
      timeout: 10_000,
      timeoutMsg: "Open in browser did not hand the Twitch link to the system browser",
    });
    await expect($$("iframe")).toBeElementsArrayOfSize(0);
  });

  it("QA.42 hides a card with the x button and keeps it hidden after a restart", async () => {
    const url = `http://127.0.0.1:${String(site.port)}/page-three`;
    await say(`dismiss me <a href="${url}">${url}</a>`);
    await waitMessage("dismiss me");
    const preview = card("Preview page-three");
    await preview.waitForDisplayed({ timeout: 15_000 });
    await preview.$("button[aria-label='Hide preview']").click();
    await preview.waitForExist({ reverse: true });
    await expect($(`//li[contains(@class,'chat-message')]//a[@href='${url}']`)).toBeDisplayed();

    await app.restart();
    await reconnectSaved();
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    await waitMessage("dismiss me");
    // Other cards load again, so the missing one is the dismissal and not a failed preview.
    await card("Preview page-one").waitForDisplayed({ timeout: 15_000 });
    await expect(card("Preview page-three")).not.toExist();
  });

  it("AC-1.45 shows an external image and a direct .png link inline via the engine", async () => {
    const base = `http://127.0.0.1:${String(site.port)}`;
    const before = site.requests.length;
    await say(
      `pictures <img src="${base}/anim.gif"> and <a href="${base}/photo.png">${base}/photo.png</a>`,
    );
    await waitMessage("pictures");
    const gif = inlineImage("anim.gif");
    const png = inlineImage("photo.png");
    await gif.waitForDisplayed({ timeout: 15_000 });
    await png.waitForDisplayed({ timeout: 15_000 });
    // The animation reaches the webview unchanged; the still image is re-encoded by the engine.
    await expect(gif.$("img")).toHaveAttribute("src", expect.stringMatching(/^data:image\/gif;base64,/));
    await expect(png.$("img")).toHaveAttribute(
      "src",
      expect.stringMatching(/^data:image\/(png|jpeg);base64,/),
    );
    const remote = await browser.execute(
      () =>
        Array.from(document.querySelectorAll(".chat-list__items img")).filter(
          (i) => !(i.getAttribute("src") ?? "").startsWith("data:"),
        ).length,
    );
    expect(remote).toBe(0);
    expect(site.requests.slice(before).sort()).toEqual(["/anim.gif", "/photo.png"]);

    // Until the image viewer exists (B42), a click opens the image in the system browser.
    await png.$("button.inline-image__view").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(`${base}/photo.png`), {
      timeout: 10_000,
      timeoutMsg: "clicking the image did not open it in the system browser",
    });
  });

  it("AC-1.39 with previews switched off shows only the plain link and makes no request", async () => {
    await openChatSettings();
    const toggle = $(
      "//input[@role='switch'][@id=//label[.='Show link previews and embeds']/@for]",
    );
    await toggle.waitForExist();
    await expect(toggle).toBeSelected();
    await toggle.click();
    await expect(toggle).not.toBeSelected();
    await browser.keys("Escape");

    const before = site.requests.length;
    const url = `http://127.0.0.1:${String(site.port)}/page-two`;
    await say(`plain <a href="${url}">${url}</a>`);
    await waitMessage("plain");
    await expect($(`//li[contains(@class,'chat-message')]//a[@href='${url}']`)).toBeDisplayed();
    // AC-1.45: neither an external image nor an image link is loaded.
    await say(
      `no pictures <img src="http://127.0.0.1:${String(site.port)}/anim-off.gif"> ` +
        `<a href="http://127.0.0.1:${String(site.port)}/photo-off.png">photo</a>`,
    );
    await waitMessage("no pictures");
    await sleep(2000);
    await expect(card("Preview page-two")).not.toExist();
    // Earlier cards and images are gone too: off means plain links everywhere.
    await expect($$("article.link-card")).toBeElementsArrayOfSize(0);
    await expect($$("figure.inline-image")).toBeElementsArrayOfSize(0);
    expect(site.requests.length).toBe(before);
  });
});
