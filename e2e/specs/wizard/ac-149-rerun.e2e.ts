import { $, $$, browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import { finishWizard } from "../../helpers/chat-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import {
  button,
  chooseSetupStyle,
  keyringDump,
  mainWindowMarker,
  setupStyleRadio,
  waitForText,
  waitForWizardStep,
  wizardPrimary,
} from "../../helpers/wizard-util.ts";

// #149 and AC-3.79 (#159): from a finished setup, Settings → About → "Run setup wizard again"
// walks Welcome → Voice → Setup style → All set, without the identity step and with every value
// prefilled. Cancel saves nothing, even after a change; Finish with only the microphone changed
// saves only the microphone. The certificate in the OS keychain is untouched on every path.

type SettingsRow = Record<string, unknown>;

/** The wizard's own dialog, so the settings page behind it cannot answer a selector. */
function wizard() {
  return $("//*[@role='dialog'][.//h1[normalize-space(.)='Setup wizard']]");
}

function wizardControl(xpath: string) {
  return wizard().$(xpath);
}

async function savedSettings(): Promise<SettingsRow> {
  return app.invoke<SettingsRow>("settings_get");
}

/** Keys whose values differ between two saved settings rows. */
function changedKeys(before: SettingsRow, after: SettingsRow): string[] {
  const keys = new Set([...Object.keys(before), ...Object.keys(after)]);
  return [...keys].filter((key) => JSON.stringify(before[key]) !== JSON.stringify(after[key])).sort();
}

async function startRerun(): Promise<void> {
  await button("Run setup wizard again").click();
  await waitForWizardStep("welcome", 20_000);
}

/** On the voice step: picks a microphone other than the selected one (the test backend lists one). */
async function changeMicrophone(): Promise<void> {
  const combo = wizardControl(".//*[@role='combobox'][@id=//label[normalize-space(.)='Microphone']/@for]");
  await combo.waitForClickable({ timeout: 10_000 });
  await combo.click();
  const others = () => $$("[role='listbox'] [role='option'][aria-selected='false']");
  await browser.waitUntil(async () => (await others().length) > 0, {
    timeout: 5_000,
    timeoutMsg: "the microphone list offers no other device",
  });
  await others()[0].click();
}

describe("#149 / AC-3.79 run the setup wizard again", () => {
  let keyringBefore = "";
  let settingsBefore: SettingsRow = {};

  before(async () => {
    await app.restart({ freshProfile: true });
    await finishWizard();
    await mainWindowMarker().waitForExist({ timeout: 30_000 });
    keyringBefore = await keyringDump();
    expect(keyringBefore).toContain("app.mumcord.client");
    settingsBefore = await savedSettings();
    await openSettingsGroup("About");
    await button("Run setup wizard again").waitForClickable({ timeout: 15_000 });
  });

  it("opens without the identity step, prefilled and one step shorter", async () => {
    await startRerun();
    await expect(wizard()).toBeDisplayed();
    expect((await waitForWizardStep("welcome")).toLowerCase()).toContain("step 1 of 9");
    await waitForText("Your identity is not changed");
    // No identity step: neither in the rail nor as a step.
    const rail = await wizard().$$(".wizard-rail__label").map((label) => label.getText());
    expect(rail).not.toContain("Identity");
    await wizardPrimary().click();
    expect((await waitForWizardStep("voice")).toLowerCase()).toContain("step 2 of 9");
    await expect(wizardControl(".//input[@autocomplete='nickname']")).not.toExist();
    // The voice step shows the saved devices, not empty selects.
    await expect(wizard().$$(".//*[@role='combobox']")).toBeElementsArrayOfSize(2);
    await wizardPrimary().click();
    await waitForWizardStep("setupStyle");
    // Prefilled: the first start chose Everything on.
    await expect(setupStyleRadio("everythingOn")).toBeSelected();
  });

  it("keeps everything on Cancel, including a change the user made", async () => {
    await chooseSetupStyle("maximumPrivacy");
    await wizardControl(".//button[normalize-space(.)='Cancel']").click();
    const question = $("//*[@role='alertdialog'][contains(., 'Discard your changes?')]");
    await question.waitForDisplayed({ timeout: 10_000 });
    await question.$("//button[normalize-space(.)='Discard changes']").click();

    await button("Run setup wizard again").waitForClickable({ timeout: 10_000 });
    await expect(wizard()).not.toExist();
    expect(changedKeys(settingsBefore, await savedSettings())).toEqual([]);
    expect(await keyringDump()).toBe(keyringBefore);
  });

  it("saves only the changed microphone on Finish and leaves the identity alone", async () => {
    await startRerun();
    await wizardPrimary().click();
    await waitForWizardStep("voice");
    await changeMicrophone();
    await wizardPrimary().click();
    await waitForWizardStep("setupStyle");
    // The prefilled preset card leads straight to the summary.
    await button("Finish setup").click();
    await waitForWizardStep("summary");
    await waitForText("Only the values you changed are saved.");
    await button("Finish").click();

    await button("Run setup wizard again").waitForClickable({ timeout: 15_000 });
    await expect(wizard()).not.toExist();
    // Focus comes back to the button that opened the wizard, so the keyboard stays where it was.
    const focused = await browser.execute(() => {
      const active = document.activeElement;
      return active instanceof HTMLElement ? active.textContent.trim() : "";
    });
    expect(focused).toBe("Run setup wizard again");
    expect(changedKeys(settingsBefore, await savedSettings())).toEqual(["inputDevice"]);
    expect(await keyringDump()).toBe(keyringBefore);
  });
});
