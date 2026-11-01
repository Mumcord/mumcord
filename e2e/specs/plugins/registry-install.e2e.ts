// Harness check for the fixture plugin registry (spec 06 B16b): the client, pointed at the
// run's registry through the test environment, lists Poll in Browse and installs it.
import { $, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { completeWizard } from "../../helpers/voice-ui.ts";

describe("harness: fixture plugin registry", () => {
  let fixtures: registry.FixtureRegistry;

  before(async function () {
    this.timeout(20 * 60_000);
    fixtures = await registry.start();
  });

  after(async () => {
    await fixtures.stop();
  });

  it("installs Poll from the fixture registry", async () => {
    await app.restart({ env: fixtures.env() });
    await completeWizard("Alice");

    // The fixture registry serves plain http on 127.0.0.1, which the client only accepts in
    // Developer mode (FR-6.74).
    await openSettingsGroup("Advanced & Developer");
    const developerMode = $("//input[@role='switch'][@id=//label[.='Developer mode']/@for]");
    await developerMode.waitForExist({ timeout: 10_000 });
    await developerMode.click();
    await expect(developerMode).toBeSelected();

    await openSettingsGroup("Plugins");
    await $("//button[@role='tab'][contains(@class,'plugins-tab')][normalize-space(.)='Browse']").click();
    const install = $("button[aria-label='Install Poll']");
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();

    // The prompt checks the package first (progress bar), then offers Install.
    const confirm = $(
      "//div[contains(concat(' ',@class,' '),' plugin-prompt ')]//button[normalize-space(.)='Install']",
    );
    await confirm.waitForClickable({ timeout: 60_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Poll installed"), {
      wait: 60_000,
    });
    await expect(
      $("//li[@class='plugin-card'][.//h3[contains(.,'Poll')]]//span[@class='plugin-card-installed']"),
    ).toBeDisplayed();
  });
});
