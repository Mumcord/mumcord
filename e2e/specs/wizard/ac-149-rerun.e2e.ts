import { $, browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import { finishWizard } from "../../helpers/chat-ui.ts";
import {
  button,
  keyringDump,
  mainWindowMarker,
  waitForText,
} from "../../helpers/wizard-util.ts";

// AC of #149: from a finished setup, Settings > About > "Run setup wizard again" walks the setup
// again without the identity step, Cancel keeps everything and finish saves only what changed.
// The certificate in the OS keychain must be untouched on every path.

/** The wizard's own dialog, so the settings page behind it cannot answer a selector. */
function wizard() {
  return $("//*[@role='dialog'][.//h1[normalize-space(.)='Setup wizard']]");
}

function wizardControl(xpath: string) {
  return wizard().$(xpath);
}

async function openAbout(): Promise<void> {
  await $("button[aria-label='Quick settings']").click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await aboutTab();
}

async function aboutTab(): Promise<void> {
  await $("//button[@role='tab' and .='About']").click();
  await button("Run setup wizard again").waitForClickable({ timeout: 15_000 });
}

/**
 * The saved update choice as Settings > Updates shows it ("Download automatically", "Notify
 * only" or "Off"). Leaves the Updates tab open.
 */
async function savedUpdateMode(): Promise<string> {
  await $("//button[@role='tab' and .='Updates']").click();
  const group = $("//*[@role='radiogroup'][@aria-labelledby=//p[.='When an update is found']/@id]");
  await group.waitForExist({ timeout: 10_000 });
  return browser.execute(() => {
    const input = document.querySelector<HTMLInputElement>(
      "[role='tabpanel']:not([hidden]) [role='radiogroup'] input[type='radio']:checked",
    );
    return input?.closest("label")?.textContent.trim() ?? "";
  });
}

/** One of the wizard's update choices ("Download automatically", "Notify only", "Off"). */
function wizardUpdateRadio(choice: string) {
  return wizardControl(`.//label[normalize-space(.)='${choice}']//input[@type='radio']`);
}

async function startRerun(): Promise<void> {
  await button("Run setup wizard again").click();
  await waitForText("Step 1 of 4", 20_000);
}

/** Walks to the update step: microphone → privacy → updates. */
async function toUpdateStep(): Promise<void> {
  await button("Next").click();
  await waitForText("Step 2 of 4");
  await button("Next").click();
  await waitForText("Step 3 of 4");
}

describe("#149 run the setup wizard again", () => {
  let keyringBefore = "";

  before(async () => {
    await app.restart({ freshProfile: true });
    await finishWizard();
    await mainWindowMarker().waitForExist({ timeout: 30_000 });
    keyringBefore = await keyringDump();
    expect(keyringBefore).toContain("app.mumcord.client");
    await openAbout();
    // The update check starts off from the finished setup.
    expect(await savedUpdateMode()).toBe("Off");
    await aboutTab();
  });

  it("opens without the identity step, prefilled and four steps long", async () => {
    await startRerun();
    await expect(wizard()).toBeDisplayed();
    // No identity step: its name field and heading never appear.
    await expect(wizardControl(".//input[@autocomplete='nickname']")).not.toExist();
    await expect(wizardControl(".//h2[normalize-space(.)='Your identity']")).not.toExist();
    await waitForText("Step 1 of 4");
    await waitForText("Your identity is not changed");
    // The microphone step shows the saved devices, not empty selects.
    await expect(wizard().$$(".//*[@role='combobox']")).toBeElementsArrayOfSize(2);
  });

  it("keeps everything on Cancel, including a change the user made", async () => {
    await toUpdateStep();
    // Prefilled with the saved choice.
    await expect(wizardUpdateRadio("Off")).toBeChecked();
    await wizardUpdateRadio("Notify only").click();
    await expect(wizardUpdateRadio("Notify only")).toBeChecked();

    await wizardControl(".//button[normalize-space(.)='Cancel']").click();
    const question = $("//*[@role='alertdialog'][contains(., 'Discard your changes?')]");
    await question.waitForDisplayed({ timeout: 10_000 });
    await question.$("//button[normalize-space(.)='Discard changes']").click();

    await button("Run setup wizard again").waitForClickable({ timeout: 10_000 });
    await expect(wizard()).not.toExist();
    // Nothing was written: Settings > Updates still shows the saved answer.
    expect(await savedUpdateMode()).toBe("Off");
    expect(await keyringDump()).toBe(keyringBefore);
    await aboutTab();
  });

  it("saves the changed update check on Finish and leaves the identity alone", async () => {
    await startRerun();
    await toUpdateStep();
    await wizardUpdateRadio("Notify only").click();
    await button("Next").click();
    await waitForText("Step 4 of 4");
    await button("Finish").click();

    await button("Run setup wizard again").waitForClickable({ timeout: 15_000 });
    await expect(wizard()).not.toExist();
    // Focus comes back to the button that opened the wizard, so the keyboard stays where it was.
    const focused = await browser.execute(() => {
      const active = document.activeElement;
      return active instanceof HTMLElement ? active.textContent.trim() : "";
    });
    expect(focused).toBe("Run setup wizard again");
    expect(await savedUpdateMode()).toBe("Notify only");
    expect(await keyringDump()).toBe(keyringBefore);
  });
});
