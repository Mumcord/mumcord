import { $, $$, browser, expect } from "@wdio/globals";
import { app } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import {
  button,
  chooseSetupStyle,
  createIdentity,
  CUSTOM_STEPS,
  dumpAppLogs,
  finishWithPreset,
  passWelcome,
  setupStyleRadio,
  voiceToSetupStyle,
  waitForWizardStep,
  WIZARD_STEP_NAMES,
  wizardPrimary,
} from "../../helpers/wizard-util.ts";

// AC-3.78 (#159): on a fresh profile the wizard shows Welcome, Identity, Voice and Setup style;
// Balanced → "Finish setup" → summary → Start leaves exactly the Balanced column of the preset
// table in Settings. Custom walks the five detailed steps (5–9) in order before the summary.

type SettingsRow = Record<string, unknown> & { privateMessages: Record<string, unknown> };

/** The settings-row part of the Balanced column (build spec #159, "Presets → settings"). */
const BALANCED = {
  "privateMessages.rule": "friendsOnly",
  "privateMessages.allowCurrentChannel": true,
  "privateMessages.allowAccepted": true,
  "privateMessages.keepRequests": true,
  p2pMode: "directRelay",
  linkPreviews: true,
  offerFullImages: true,
  loadFullImages: true,
  historySharing: true,
  nowPlaying: false,
  nowGaming: false,
};

function valueOf(settings: SettingsRow, key: string): unknown {
  return key.startsWith("privateMessages.")
    ? settings.privateMessages[key.slice("privateMessages.".length)]
    : settings[key];
}

/** The header of the shown step, without the case the CSS gives it. */
async function header(step: Parameters<typeof waitForWizardStep>[0]): Promise<string> {
  return (await waitForWizardStep(step)).toLowerCase();
}

describe("AC-3.78 setup wizard with presets", () => {
  afterEach(function () {
    if (this.currentTest?.state === "failed") dumpAppLogs(app.profile());
  });

  it("starts with Welcome, Identity, Voice and Setup style, Balanced preselected", async () => {
    expect(await header("welcome")).toContain("step 1 of 5");
    await passWelcome();
    expect(await header("identity")).toContain("step 2 of 5");
    await createIdentity("PresetTester");
    await voiceToSetupStyle();
    expect(await header("setupStyle")).toContain("step 4 of 5");
    await expect(setupStyleRadio("balanced")).toBeSelected();
    const cards = await $$("form.wizard-step label.preset-card").map((card) =>
      card.getAttribute("data-preset"),
    );
    expect(cards).toEqual(["everythingOn", "balanced", "maximumPrivacy", "custom"]);
  });

  it("Balanced → Finish setup → summary → main window", async () => {
    await chooseSetupStyle("balanced");
    await expect(wizardPrimary()).toHaveText(expect.stringContaining("Finish setup"));
    await finishWithPreset("balanced");
  });

  it("saves the Balanced column and Settings shows Balanced without changes", async () => {
    const settings = await app.invoke<SettingsRow>("settings_get");
    for (const [key, value] of Object.entries(BALANCED)) {
      expect({ key, value: valueOf(settings, key) }).toEqual({ key, value });
    }
    expect(settings.setupPreset).toBe("balanced");

    await openSettingsGroup("Privacy & Connections");
    await expect($(".active-preset__name")).toHaveText("Balanced");
    // The count covers the in-game overlay and Do Not Disturb as well.
    await expect($(".active-preset__count")).toHaveText("No changes since setup");
    await expect($("fieldset.p2p-modes input[value='directRelay']")).toBeSelected();
    await openSettingsGroup("Chat & Notifications");
    await expect($("input[type='radio'][value='friendsOnly']")).toBeSelected();
    await browser.keys(["Escape"]);
  });
});

describe("AC-3.78 setup wizard, Custom path", () => {
  before(async () => {
    await app.restart({ freshProfile: true });
  });

  it("walks Private messages, Connections, Features, Appearance and Startup, then the summary", async () => {
    await passWelcome();
    await createIdentity("CustomTester");
    await voiceToSetupStyle();
    await chooseSetupStyle("custom");
    await expect(wizardPrimary()).toHaveText(expect.stringContaining("Next"));
    await wizardPrimary().click();
    // Welcome, Identity, Voice, Setup style, the five Custom steps, All set.
    for (const [index, step] of CUSTOM_STEPS.entries()) {
      expect(await header(step)).toContain(`step ${String(index + 5)} of 10`);
      const tagged = $(
        `//li[contains(@class,'wizard-rail__step')][@aria-current='step'][.//span[contains(@class,'wizard-rail__label')][normalize-space(.)='${WIZARD_STEP_NAMES[step]}']]//span[contains(@class,'wizard-rail__tag')]`,
      );
      await expect(tagged).toHaveText("Custom");
      await wizardPrimary().click();
    }
    expect(await header("summary")).toContain("step 10 of 10");
    await button("Start Mumcord").click();
    await $('button[aria-label="Add a server"]').waitForExist({ timeout: 30_000 });
    // Untouched Custom values are the Balanced ones the wizard starts from, so Balanced is recorded.
    const settings = await app.invoke<SettingsRow>("settings_get");
    expect(settings.setupPreset).toBe("balanced");
  });
});
