// Harness check for the fixture plugin registry (spec 06 B16b): the client, pointed at the
// run's registry through the test environment, lists Poll in Browse and installs it.
import { $, expect } from "@wdio/globals";
import { app, registry } from "../../helpers/index.ts";
import { openSettingsGroup, settingsCard } from "../../helpers/settings-ui.ts";
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
    await openSettingsGroup("Advanced & Developer");
    await settingsCard("Plugins").waitForDisplayed({ timeout: 10_000 });
    await $("//button[@role='tab'][normalize-space(.)='Browse']").click();
    const install = $("button[aria-label='Install Poll']");
    await install.waitForClickable({ timeout: 30_000 });
    await install.click();
    const confirm = $("//div[contains(@class,'plugin-prompt')]//button[normalize-space(.)='Install']");
    await confirm.waitForClickable({ timeout: 30_000 });
    await confirm.click();
    await expect($(".plugin-toast")).toHaveText(expect.stringContaining("Poll installed"), {
      wait: 30_000,
    });
  });
});
