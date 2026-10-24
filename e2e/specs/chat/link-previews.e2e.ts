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
  selectChannel,
  sleep,
  waitMessage,
} from "../../helpers/chat-ui.ts";

/** 1×1 red PNG as the page's preview image. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC",
  "base64",
);

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
    if (path === "/cover.png") {
      res.writeHead(200, { "Content-Type": "image/png" }).end(PNG);
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

async function say(html: string): Promise<void> {
  await bot(["say", "--channel", "Lobby", html], { name: "LinkBot", hold: 1 });
}

async function openChatSettings(): Promise<void> {
  const button = $("button[aria-label='Settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='tab' and .='Chat & history']").click();
}

// AC-1.39 (link preview card, switch off → plain link and no request) and AC-1.40 (YouTube and
// Twitch player cards that load nothing before a click).
describe("chat: link previews and player cards (AC-1.39, AC-1.40)", () => {
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
    await sleep(2000);
    await expect(card("Preview page-two")).not.toExist();
    // Earlier cards are gone too: off means plain links everywhere.
    await expect($$("article.link-card")).toBeElementsArrayOfSize(0);
    expect(site.requests.length).toBe(before);
  });
});
