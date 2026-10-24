import { expect, $ } from "@wdio/globals";
import { testenv } from "../../helpers/index.ts";
import { addFirstServer, finishWizard, selectChannel } from "../../helpers/chat-ui.ts";

// AC-1.16 (old server): Mumble 1.3 has no channel listeners, so Follow is hidden with a hint.
describe("chat: follow on a 1.3 server (AC-1.16)", () => {
  before(async () => {
    await testenv.up("v13");
    await finishWizard();
    await addFirstServer(testenv.ports.v13);
  });

  it("hides Follow and shows the version hint", async () => {
    await selectChannel("Raid");
    await expect($("//button[contains(@class,'follow-toggle') and @aria-pressed]")).not.toExist();
    const hint = $("p.follow-hint");
    await expect(hint).toBeDisplayed();
    await expect(hint).toHaveText(expect.stringContaining("1.4 or newer"));
  });
});
