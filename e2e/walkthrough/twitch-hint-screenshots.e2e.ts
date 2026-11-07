// Temporary screenshot run for issue #419: the Twitch card on Linux (dark and light). Not part of
// the suite; it exists to produce the two pictures for the chat card's browser hint.
import { cpSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $ } from "@wdio/globals";
import { app, bot, testenv } from "../helpers/index.ts";
import { artifactsDir, repoRoot } from "../helpers/paths.ts";
import {
  addFirstServer,
  finishWizard,
  joinChannel,
  selectChannel,
  waitMessage,
} from "../helpers/chat-ui.ts";

const SHOT_DIR = join(repoRoot, "docs", "images", "screenshots");

async function shot(name: string): Promise<void> {
  const file = join(artifactsDir, `${name}.png`);
  await browser.saveScreenshot(file);
  cpSync(file, join(SHOT_DIR, `${name}.png`));
  console.log(`screenshot: ${join(SHOT_DIR, `${name}.png`)}`);
}

/** Picks Dark or Light in the appearance settings and comes back to the chat. */
async function chooseMode(label: string): Promise<void> {
  await $('button[aria-label="Quick settings"]').click();
  await $(
    "//*[contains(@class,'quick-settings')]//button[contains(normalize-space(.),'All settings')]",
  ).click();
  await $('[role="tab"]=Appearance').click();
  const combo = $("//label[normalize-space(.)='Dark or light']/following::*[@role='combobox'][1]");
  await combo.click();
  await $(`//*[@role='option' and normalize-space(.)='${label}']`).click();
  await browser.pause(800);
  await browser.keys(["Escape"]);
  await $(".overlay__dialog").waitForExist({ reverse: true, timeout: 10_000 });
  await browser.pause(500);
}

function card(title: string) {
  return $(
    `//article[contains(@class,'link-card')][.//p[@class='link-card__title' and .='${title}']]`,
  );
}

describe("screenshots: Twitch card on Linux (issue #419)", function () {
  this.timeout(240_000);

  before(async () => {
    mkdirSync(SHOT_DIR, { recursive: true });
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await finishWizard("Shots");
    await addFirstServer(testenv.ports.v15);
    await joinChannel("Lobby");
    await selectChannel("Lobby");
    await chooseMode("Dark");
    await bot(
      ["say", "--channel", "Lobby", '<a href="https://www.twitch.tv/mumcordtest">watch this</a>'],
      { name: "ShotBot", hold: 1 },
    );
    await waitMessage("watch this");
    await card("Twitch video").waitForDisplayed({ timeout: 15_000 });
  });

  it("shoots the card with its hint, dark and light", async () => {
    await browser.pause(1_000);
    await shot("video-embed-twitch-hint-dark");
    await chooseMode("Light");
    await shot("video-embed-twitch-hint-light");
  });
});
