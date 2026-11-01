import { cpSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $ } from "@wdio/globals";
import { app, axe } from "../helpers/index.ts";
import { artifactsDir, repoRoot } from "../helpers/paths.ts";
import { hasClass } from "../helpers/selectors.ts";
import {
  ariaButton,
  completeWizard,
  dialogButtonIn,
  dialogTitle,
  fill,
  fillIn,
  openSettingsSection,
  pickOption,
  settingsAction,
  settingsTitle,
} from "../helpers/servers-ui.ts";

// Manual acceptance walkthrough for issue #195: the screens are walked as a person would, and the
// pictures of the running client go to docs/images/screenshots (dark and light for every UI change).
const SHOT_DIR = join(repoRoot, "docs", "images", "screenshots");

async function shot(name: string): Promise<void> {
  const file = join(artifactsDir, `${name}.png`);
  await browser.saveScreenshot(file);
  cpSync(file, join(SHOT_DIR, `${name}.png`));
  console.log(`screenshot: ${join(SHOT_DIR, `${name}.png`)}`);
}

/** Emulates the system's 200 % text scaling (the app follows the root font size). */
async function scale(factor: number): Promise<void> {
  await browser.execute((value: number) => {
    document.documentElement.style.setProperty("font-size", String(value * 16) + "px");
  }, factor);
  await browser.pause(600);
}

async function chooseMode(mode: string): Promise<void> {
  await openSettingsSection("appearance");
  await pickOption("Dark or light", mode);
  await browser.pause(1_000);
}

async function createIdentity(name: string, label: string): Promise<void> {
  await settingsAction("New identity").click();
  await settingsTitle("New identity").waitForDisplayed();
  await fillIn("New identity", "Name on servers", name);
  await fillIn("New identity", "Label", label);
  await dialogButtonIn("New identity", "Create").click();
  await $(
    `//li[${hasClass("identity-card")}][.//strong[normalize-space()="${label}"]]`,
  ).waitForDisplayed({ timeout: 60_000 });
}

describe("manual acceptance walkthrough (issue #195)", function () {
  this.timeout(240_000);

  before(async () => {
    mkdirSync(SHOT_DIR, { recursive: true });
    await app.restart({ freshProfile: true });
    await completeWizard("Walkthrough");
    await chooseMode("Dark"); // the CI desktop follows the system (light), so name the theme here
  });

  it("identity manager: two cards, the details, dark and light", async () => {
    await openSettingsSection("account");
    await createIdentity("walkthrough-a", "Walkthrough Alice");
    await createIdentity("walkthrough-b", "Walkthrough Bob");

    const violations = await axe.scan("identity-manager");
    console.log(`axe on the identity section: ${axe.summarize(violations) || "no violations"}`);
    for (const violation of violations) {
      for (const node of violation.nodes) {
        console.log(`  ${violation.id}: ${node.target.join("")}`);
      }
    }
    await shot("settings-identity-walkthrough-dark");

    await ariaButton("Show details of Walkthrough Bob").click();
    await settingsTitle("Identity Walkthrough Bob").waitForDisplayed({ timeout: 10_000 });
    await shot("settings-identity-details-walkthrough-dark");
    await browser.keys(["Escape"]);

    // The UI change: the card's action row wraps at 200 % text instead of clipping.
    await scale(2);
    await browser.execute(() => {
      document.querySelector(".identity-card")?.scrollIntoView({ block: "end" });
    });
    await browser.pause(400);
    await shot("settings-identity-walkthrough-200-dark");
    await chooseMode("Light");
    await openSettingsSection("account");
    await openSettingsSection("account");
    await browser.execute(() => {
      document.querySelector(".identity-card")?.scrollIntoView({ block: "end" });
    });
    await browser.pause(400);
    await shot("settings-identity-walkthrough-200-light");
    await scale(1);
    await chooseMode("Dark");
  });

  it("add and edit server dialog: dark and light", async () => {
    const link = "mumble://walk:qa-secret@localhost:64000/Raid?channelTitle=Quality";
    // The dialog comes from the server bar, so the settings overlay has to be closed first.
    await browser.keys(["Escape"]);
    await $('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed({ timeout: 15_000 });
    await fill("Address", link);
    await fill("Name in the server bar", "Walkthrough server");
    await shot("server-dialog-connection-walkthrough-dark");

    await $('//div[@role="tablist"]//button[@role="tab"][normalize-space()="Identity"]').click();
    await shot("server-dialog-identity-walkthrough-dark");
    await browser.keys(["Escape"]);

    await chooseMode("Light");
    await browser.keys(["Escape"]);
    await $('button[aria-label="Add a server"]').click();
    await dialogTitle("Add a server").waitForDisplayed({ timeout: 15_000 });
    await fill("Address", link);
    await fill("Name in the server bar", "Walkthrough server");
    await shot("server-dialog-connection-walkthrough-light");

    await $('//div[@role="tablist"]//button[@role="tab"][normalize-space()="Behaviour"]').click();
    await shot("server-dialog-behaviour-walkthrough-light");
    await browser.keys(["Escape"]);
    await chooseMode("Dark");
  });
});
