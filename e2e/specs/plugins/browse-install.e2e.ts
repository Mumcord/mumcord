// AC-6.1 and AC-6.30 (spec 06): one app against the fixture registry and the local test server.
// Browse puts Echolings first and nothing is installed; installing Poll shows the coloured
// permission rows, the visibility choice and the progress bar, then a toast and the Poll icon in
// the Plugins strip. `mumcord://plugins/dice` opens Dice's details without installing it, and a
// link to a third-party plugin id does nothing.
import { $, $$, browser, expect } from "@wdio/globals";
import { app, registry, testenv } from "../../helpers/index.ts";
import { fill, launchWithLink } from "../../helpers/servers-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard } from "../../helpers/voice-ui.ts";

/** A third-party plugin of the fixture repository (spec 06 §11). */
const THIRD_PARTY_ID = "fx-dice-like";

const pluginTab = (name: string) =>
  $(`//button[@role='tab'][contains(@class,'plugins-tab')][starts-with(normalize-space(.),'${name}')]`);
const prompt = () => $("//div[contains(concat(' ',@class,' '),' plugin-prompt ')]");
const details = (id: string) => $(`article.pd[aria-labelledby='plugin-details-${id}']`);
const stripIcon = (name: string) => $(`.pstrip-icon[aria-label='${name}']`);
const settingsOverlay = () => $(".overlay__dialog");

async function openPlugins(tab: string): Promise<void> {
  await openSettingsGroup("Plugins");
  await pluginTab(tab).click();
}

async function closeSettings(): Promise<void> {
  await browser.keys(["Escape"]);
  await settingsOverlay().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Sends a link the way the OS does while Mumcord runs: a second launch forwards it. */
async function sendLink(link: string): Promise<void> {
  const second = launchWithLink(link);
  await new Promise<void>((resolve) => {
    if (second.exitCode !== null) resolve();
    else
      second.once("exit", () => {
        resolve();
      });
  });
}

describe("plugins: Browse, install prompt and deep links (AC-6.1, AC-6.30)", () => {
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    await testenv.up("v15");
    fixtures = await registry.start();
    await app.restart({ env: fixtures.env() });
    await completeWizard("Alice");
    // The Plugins strip lives in the channel panel, so the app is connected.
    await addAndConnect();
    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();
    await closeSettings();
  });

  after(async () => {
    await fixtures.stop();
  });

  it("AC-6.1: nothing is installed and Browse lists Echolings first", async () => {
    await openPlugins("Installed");
    await expect($("//p[normalize-space(.)='No plugins yet.']")).toBeDisplayed({ wait: 10_000 });
    await pluginTab("Browse").click();
    await $("button[aria-label='Install Poll']").waitForClickable({ timeout: 30_000 });
    const names = await $$("ul.plugin-browse-list > li.plugin-card .plugin-card-name").map((el) =>
      el.getText(),
    );
    expect(names[0]).toContain("Echolings");
    expect(names.join(" | ")).toEqual(expect.stringContaining("Poll"));
    await expect($(".plugin-card-installed")).not.toBeExisting();
  });

  it("AC-6.1: the install prompt shows coloured required permissions and the visibility choice", async () => {
    // The prompt's progress bar (checking, then installing) is brief locally, so a DOM observer
    // records whether it ever appeared.
    await browser.execute(() => {
      const w = window as unknown as { __sawProgress?: boolean };
      w.__sawProgress = false;
      const seen = () => document.querySelector(".plugin-prompt [role='progressbar']") !== null;
      const observer = new MutationObserver(() => {
        if (seen()) {
          w.__sawProgress = true;
          observer.disconnect();
        }
      });
      observer.observe(document.body, { childList: true, subtree: true });
    });
    await $("button[aria-label='Install Poll']").click();
    await prompt().waitForDisplayed({ timeout: 30_000 });
    const rows = $$("ul.perm-rows > li.perm-row");
    await rows[0].waitForDisplayed({ timeout: 30_000 });
    // Poll's five required permissions each carry the "Required" chip.
    expect(await $$("ul.perm-rows .perm-chip").length).toBe(5);
    // Every row shows its risk colour; Poll asks for green and yellow ones, drawn differently.
    const risks = await $$("ul.perm-rows .perm-icon").map(async (icon) => {
      const cls = (await icon.getAttribute("class")) ?? "";
      const risk = /perm-risk-(green|yellow|red)/.exec(cls)?.[1];
      const colour = await browser.execute(
        (el) => getComputedStyle(el as unknown as Element).color,
        icon as unknown as HTMLElement,
      );
      return { risk, colour };
    });
    expect(risks.every((r) => r.risk !== undefined)).toBe(true);
    const green = risks.find((r) => r.risk === "green");
    const yellow = risks.find((r) => r.risk === "yellow");
    expect(green).toBeDefined();
    expect(yellow).toBeDefined();
    expect(green?.colour).not.toEqual(yellow?.colour);
    // Poll talks to peers, so the prompt asks who may tell; the manifest default is Everyone.
    const group = $(".plugin-prompt-visibility [role='radiogroup']");
    await expect(group).toBeDisplayed();
    const choices = await group.$$("input[type='radio']").map((r) => r.getAttribute("value"));
    expect(choices).toEqual(["everyone", "friends", "nobody"]);
    await expect(group.$("input[value='everyone']")).toBeSelected();
  });

  it("AC-6.1: Install shows the progress bar, a toast and the Poll icon in the strip", async () => {
    const install = prompt().$("button=Install");
    await install.waitForClickable({ timeout: 60_000 });
    await install.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Poll installed"), {
      wait: 30_000,
    });
    const sawProgress = await browser.execute(
      () => (window as unknown as { __sawProgress?: boolean }).__sawProgress === true,
    );
    expect(sawProgress).toBe(true);
    await closeSettings();
    await expect(stripIcon("Poll")).toBeDisplayed({ wait: 30_000 });
  });

  it("AC-6.30: mumcord://plugins/dice opens Dice in Browse without installing it", async () => {
    await sendLink("mumcord://plugins/dice");
    await details("dice").waitForDisplayed({ timeout: 30_000 });
    await expect(details("dice").$("button[aria-label='Install Dice roller']")).toBeDisplayed();
    await expect($(".plugin-toast*=Dice installed")).not.toBeExisting();
    await details("dice").$("button.pd-back").click();
    await closeSettings();
  });

  it("AC-6.30: a link to a third-party plugin id does nothing", async () => {
    // Add the fixture repository so the id really is a listed third-party plugin.
    await openPlugins("Repositories");
    await $("button=Add repository").click();
    await fill("Repository address", fixtures.repoUrl);
    await $("button=Check repository").click();
    const confirm = $("button.settings-button--danger=Add repository");
    await confirm.waitForClickable({ timeout: 30_000 });
    await confirm.click();
    await confirm.waitForDisplayed({ reverse: true, timeout: 10_000 });
    await pluginTab("Browse").click();
    await browser.waitUntil(async () => (await $$(".plugin-label-unofficial").length) > 0, {
      timeout: 30_000,
      timeoutMsg: "the third-party repository's plugins never appeared in Browse",
    });
    await closeSettings();

    await sendLink(`mumcord://plugins/${THIRD_PARTY_ID}`);
    // The link is handled within a second or two; give it ample time to (not) open anything.
    await browser.pause(5_000);
    await expect(settingsOverlay()).not.toBeDisplayed();
    await expect(details(THIRD_PARTY_ID)).not.toBeExisting();

    // The same path still works for an official id afterwards, so the link really arrived.
    await sendLink("mumcord://plugins/dice");
    await details("dice").waitForDisplayed({ timeout: 30_000 });
  });
});
