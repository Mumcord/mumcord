// Issue #419: the controls Mumcord puts on an embedded YouTube or Twitch player.
//
// "Copy link" goes through the engine to the system clipboard, because a cross-origin player page
// cannot copy. Full screen: only the frames Mumcord grants the permission may ask for it, and the
// card's toolbar lives inside the element that goes full screen, so a click puts the player on the
// screen and a second click takes it back. Twitch's own player refuses to run under the app's page
// on macOS and Linux, so a Twitch card says that and hands the stream to the browser. What the window itself
// does on a real desktop (KDE, GNOME, Windows, macOS) is the manual check of the issue; here the
// harness checks the state the page reports.
import { browser, expect, $ } from "@wdio/globals";
import { app, bot, testenv } from "../../helpers/index.ts";
import { readFileSync } from "node:fs";
import {
  addFirstServer,
  finishWizard,
  installOpenerStub,
  joinChannel,
  readClipboard,
  selectChannel,
  waitMessage,
} from "../../helpers/chat-ui.ts";

function card(title: string) {
  return $(
    `//article[contains(@class,'link-card')][.//p[@class='link-card__title' and .='${title}']]`,
  );
}

/**
 * A card keeps its picture box before it plays as well: the picture is then its thumbnail, so a
 * second card in the message list carries the same class. The selector names the card by the link
 * it came from, which is what the app puts on it.
 */
function boxOf(url: string): string {
  return `article[data-link-url='${url}'] .link-card__player-box`;
}

async function say(html: string): Promise<void> {
  await bot(["say", "--channel", "Lobby", html], { name: "EmbedBot", hold: 1 });
}

/** Whether the page's full screen is on that card's box. */
function onScreen(url: string): Promise<boolean> {
  return browser.execute(
    (sel: string) => document.fullscreenElement === document.querySelector(sel),
    boxOf(url),
  );
}

/**
 * Press a button of that card's overlay bar. The bar fades in on hover or keyboard focus, so the
 * click is repeated until the page reports the state a click should have produced.
 */
async function press(url: string, label: string): Promise<void> {
  const button = await $(`article[data-link-url='${url}'] button[aria-label='${label}']`);
  await button.waitForExist({ timeout: 10_000 });
  await button.click();
}

/** Wait for the page to report full screen on, or off, clicking the card's button until it does. */
async function untilScreen(url: string, target: boolean, why: string): Promise<void> {
  await browser.waitUntil(
    async () => {
      if ((await onScreen(url)) === target) return true;
      await press(url, target ? "Full screen" : "Leave full screen");
      return false;
    },
    { interval: 1_000, timeout: 15_000, timeoutMsg: why },
  );
}

describe("chat: full screen and copy link on an embed (issue #419)", () => {
  let opener: { log: string; binDir: string };

  before(async () => {
    await testenv.up("v15");
    opener = installOpenerStub();
    process.env.PATH = `${opener.binDir}:${process.env.PATH ?? ""}`;
    await app.restart();
    await finishWizard();
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
  });

  it("copies the canonical video link, without the parameters the player adds", async () => {
    const youtube = "https://youtu.be/dQw4w9WgXcQ?si=abc";
    await say(`<a href="${youtube}">embed me</a>`);
    await waitMessage("embed me");
    const preview = card("YouTube video");
    await preview.waitForDisplayed({ timeout: 15_000 });
    await press(youtube, "Copy link");
    await browser.waitUntil(
      async () => (await readClipboard("text")) === "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      { timeout: 10_000, timeoutMsg: "the card did not put the link on the clipboard" },
    );
  });

  it("shows the player full screen and takes it back with the same click", async () => {
    const youtube = "https://www.youtube.com/watch?v=aaaaaaaaaaa";
    await say(`<a href="${youtube}">play it</a>`);
    await waitMessage("play it");
    // Two YouTube cards share the fallback title, so this one is named by the link it came from.
    const preview = $(`article[data-link-url='${youtube}']`);
    await preview.waitForDisplayed({ timeout: 15_000 });
    // Before it plays, the card's picture is the thumbnail of the same overlay bar (issue #163).
    await preview.$("button[aria-label^='Play']").click();

    const box = await $(boxOf(youtube));
    await box.waitForExist({ timeout: 15_000 });
    const frame = await $(`article[data-link-url='${youtube}'] iframe.link-card__player`);
    // The permission the player's own button needs, and nothing more: no popups, no top navigation.
    expect(await frame.getAttribute("allow")).toContain("fullscreen");
    expect(await frame.getAttribute("allowfullscreen")).toBe("true");
    expect(await frame.getAttribute("sandbox")).not.toContain("allow-popups");

    // The toolbar is inside the full-screen element, so it can be clicked there.
    const toolbar = await box.$("[role='toolbar']");
    await toolbar.waitForExist({ timeout: 10_000 });
    await press(youtube, "Full screen");
    await untilScreen(youtube, true, "the player did not go full screen");
    await press(youtube, "Leave full screen");
    await untilScreen(youtube, false, "the click did not leave full screen");
    // Escape is the other way out, and the player's own button does the same from inside.
    await press(youtube, "Full screen");
    await untilScreen(youtube, true, "no full screen after the second click");
    await browser.keys("Escape");
    await untilScreen(youtube, false, "Escape did not end full screen");
  });

  // Twitch's embed only plays when every page above it is http(s); in Mumcord's window the top page
  // is an app page on macOS and Linux, so the card keeps its picture, names the reason and hands the
  // stream to the browser instead of showing a box that stays black.
  it("says that Twitch plays in the browser and hands it there", async () => {
    const twitch = "https://www.twitch.tv/mumcordtest";
    await say(`<a href="${twitch}">watch it</a>`);
    await waitMessage("watch it");
    const preview = card("Twitch video");
    await preview.waitForDisplayed({ timeout: 15_000 });
    await expect(preview.$("p.link-card__browser-hint")).toHaveText(
      expect.stringContaining("does not run inside Mumcord's window"),
    );
    expect(await preview.$("iframe").isExisting()).toBe(false);

    await preview.$("button[aria-label^='Play']").click();
    await browser.waitUntil(() => readFileSync(opener.log, "utf8").includes(twitch), {
      timeout: 10_000,
      timeoutMsg: "Play did not hand the Twitch link to the system browser",
    });
    expect(await preview.$("iframe").isExisting()).toBe(false);
  });
});
