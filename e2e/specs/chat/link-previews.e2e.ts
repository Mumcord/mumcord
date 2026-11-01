import { createServer, type Server } from "node:http";
import { readFileSync } from "node:fs";
import type { AddressInfo } from "node:net";
import { browser, expect, $, $$ } from "@wdio/globals";
import { app, bot, botInBackground, testenv } from "../../helpers/index.ts";
import {
  addFirstServer,
  expectBlobImage,
  finishWizard,
  installOpenerStub,
  joinChannel,
  reconnectSaved,
  selectChannel,
  sleep,
  waitMessage,
} from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";

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
  0x00, 0x00, 0x02, 0x02, 0x4c, 0x01, 0x00, 0x3b,
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
  return $(
    `//article[contains(@class,'link-card')][.//p[@class='link-card__title' and .='${title}']]`,
  );
}

function inlineImage(name: string) {
  return $(`figure.inline-image[aria-label='Image: ${name}']`);
}

async function say(html: string): Promise<void> {
  await bot(["say", "--channel", "Lobby", html], { name: "LinkBot", hold: 1 });
}

async function openChatSettings(): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
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
    await addFirstServer(testenv.ports.v15);
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
    await expect(image).toHaveAttribute(
      "src",
      expect.stringMatching(/^data:image\/(png|jpeg);base64,/),
    );
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

    // FR-2.41a: play runs the video inline through the app's loopback player page; nothing is
    // handed to the system browser.
    await yt.$("button.link-card__play").click();
    await $("iframe[title^='Video player']").waitForExist({ timeout: 15_000 });
    expect(readFileSync(opener.log, "utf8")).not.toContain(youtube);
    // The player iframe appearing above shifts the Twitch card, so a single click can land
    // on the old position; click again until the opener sees the link.
    await browser.waitUntil(
      async () => {
        await tw.$("button.link-card__open").click();
        return readFileSync(opener.log, "utf8").includes(twitch);
      },
      {
        interval: 1_000,
        timeout: 10_000,
        timeoutMsg: "Open in browser did not hand the Twitch link to the system browser",
      },
    );
    await expect($$("iframe")).toBeElementsArrayOfSize(1);
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
    // Both are shown through blob: URLs of the engine's bytes (#109), so the type is not visible
    // here; the engine tests check that the animation passes unchanged and stills are re-encoded.
    await expectBlobImage(gif.$("img"));
    await expectBlobImage(png.$("img"));
    const remote = await browser.execute(
      () =>
        Array.from(document.querySelectorAll(".chat-list__items img")).filter(
          (i) => !/^(data|blob):/.test(i.getAttribute("src") ?? ""),
        ).length,
    );
    expect(remote).toBe(0);
    expect(site.requests.slice(before).sort()).toEqual(["/anim.gif", "/photo.png"]);

    // FR-1.56 / AC-1.46: a click opens the viewer; + zooms, → shows the next image, Escape closes
    // and returns focus to the clicked image; "Open in browser" hands the address to the system.
    await gif.$("button.inline-image__view").click();
    const viewer = $("[role='dialog'][aria-label='Image viewer']");
    await viewer.waitForDisplayed();
    await expect($(".image-viewer__counter")).toHaveText("1 / 2");
    const scale = () =>
      browser.execute(() => {
        const match = /scale\(([\d.]+)\)/.exec(
          (document.querySelector(".image-viewer__img") as HTMLElement).style.transform,
        );
        return Number(match?.[1] ?? 0);
      });
    await browser.waitUntil(async () => (await scale()) > 0);
    const fitted = await scale();
    await browser.keys("+");
    await browser.waitUntil(async () => (await scale()) > fitted, { timeoutMsg: "+ did not zoom" });
    await browser.keys("ArrowRight");
    await expect($(".image-viewer__counter")).toHaveText("2 / 2");
    await $("button[aria-label='Open in browser']").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(`${base}/photo.png`), {
      timeout: 10_000,
      timeoutMsg: "Open in browser in the viewer did not reach the system browser",
    });
    await browser.keys("Escape");
    await viewer.waitForDisplayed({ reverse: true });
    await browser.waitUntil(
      () =>
        browser.execute(
          () => document.activeElement?.classList.contains("inline-image__view") ?? false,
        ),
      { timeoutMsg: "focus did not return to the clicked image" },
    );
  });

  // FR-1.52 / AC-1.42 (QA.43): no GIF search; GIFs from the chat are saved as favorites and sent
  // again from the GIF button as a plain link.
  it("AC-1.42 saves a GIF as favorite, sends it from the GIF button, keeps it across a restart", async () => {
    const url = `http://127.0.0.1:${String(site.port)}/anim.gif`;
    const gif = inlineImage("anim.gif");
    await gif.waitForDisplayed({ timeout: 15_000 });
    // The test GIF is 1×1 px, too small for the hover buttons; the viewer toolbar has the star.
    await gif.$("button.inline-image__view").click();
    const viewer = $("[role='dialog'][aria-label='Image viewer']");
    await viewer.waitForDisplayed();
    await viewer.$("button[aria-label='Save as favorite']").click();
    await viewer.$("button[aria-label='Remove from favorites']").waitForExist();
    await browser.keys("Escape");
    await viewer.waitForDisplayed({ reverse: true });

    // A channel message only reaches people in that channel, so the bot joins Lobby first.
    const listener = botInBackground(["script"], { name: "GifListener", timeout: 30 });
    listener.send("join Lobby");
    listener.send(`expect message --channel Lobby --contains ${url}`);
    listener.process.stdin?.end();
    await sleep(3000);
    await $("button[aria-label='GIF favorites']").click();
    const picker = $("[role='dialog'][aria-label='GIF favorites']");
    await picker.waitForDisplayed();
    const choose = picker.$("button[aria-label='Send anim.gif']");
    await expectBlobImage(choose.$("img"));
    await choose.click();
    await listener.done;
    await picker.waitForDisplayed({ reverse: true });

    await app.restart();
    await reconnectSaved();
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    await $("button[aria-label='GIF favorites']").click();
    const again = $("[role='dialog'][aria-label='GIF favorites']");
    await again.waitForDisplayed();
    await again.$("button[aria-label='Send anim.gif']").waitForDisplayed();
    await again.$("button[aria-label='Remove anim.gif from favorites']").click();
    await expect(again).toHaveText(expect.stringContaining("No favorites yet"));
    await browser.keys("Escape");
    await again.waitForDisplayed({ reverse: true });
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

    // Issue #261: each message names what was hidden in one hint that leads to the switch.
    const tail = "hidden because link previews and embeds are off in Settings > Privacy.";
    const hint = (text: string) =>
      $(`//li[contains(@class,'chat-message')][contains(., '${text}')]//p[@class='embed-hint']`);
    await expect(hint("plain")).toHaveText(`Link preview ${tail}`);
    await expect(hint("no pictures")).toHaveText(`2 previews ${tail}`);
    await hint("no pictures").$("button=Settings > Privacy").click();
    await expect($("//button[@role='tab' and .='Privacy & Connections']")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await browser.keys("Escape");
    await $(".overlay__dialog").waitForExist({ reverse: true, timeout: 5000 });
  });

  // Issue #160: the watch party loads YouTube, so it follows the same switch.
  it("with embeds switched off Watch together is disabled and names the privacy settings", async () => {
    const apps = $("button[aria-label='Apps']");
    await apps.waitForClickable();
    await apps.click();
    const watch = $("//button[@role='menuitem'][starts-with(., 'Watch together')]");
    await watch.waitForDisplayed();
    await expect(watch).toHaveAttribute("aria-disabled", "true");
    await expect(watch).toHaveText(
      expect.stringContaining("Turn on embedded players in Privacy settings to watch together"),
    );
    const link = $("//button[@role='menuitem' and .='Open Privacy settings']");
    await link.click();
    await expect($("//button[@role='tab' and .='Privacy & Connections']")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });
});
