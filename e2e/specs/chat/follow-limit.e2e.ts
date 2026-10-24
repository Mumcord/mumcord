import { expect, $ } from "@wdio/globals";
import { bot, testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard, selectChannel, USER } from "../../helpers/chat-ui.ts";

// AC-1.16 (listener limit): the `limit` server refuses every Listen request.
describe("chat: follow on a server with listener limit 0 (AC-1.16)", () => {
  before(async () => {
    await testenv.up("limit");
    await finishWizard();
    await addFirstServer(testenv.ports.limit);
  });

  it("Follow shows a clear error and does not follow", async () => {
    await selectChannel("Raid");
    const follow = $("//button[contains(@class,'follow-toggle') and @aria-pressed]");
    await follow.click();
    const error = $("p.follow-error[role='alert']");
    await error.waitForDisplayed({ timeout: 10_000 });
    const text = await error.getText();
    console.warn(`AC-1.16 limit error: ${text}`);
    expect(text).toMatch(/can't follow more channels|doesn't allow you to follow/);
    await expect(follow).toHaveAttribute("aria-pressed", "false");
    await bot(["expect", "user", USER, "--not-listening", "Raid"], {
      name: "LimitBot",
      port: testenv.ports.limit,
    });
  });
});
