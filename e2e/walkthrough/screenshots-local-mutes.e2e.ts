// Screenshots of feature 15, AC-15.8 (issue #872): Settings → Chat & Notifications with the
// "Local mutes" group (switch "Forget local mutes when Mumcord starts") directly above
// "Blocked users", in dark and light.
//
// Unlike the other walkthroughs, this one runs at 1x: the committed pictures
// docs/images/screenshots/15-local-mutes-{dark,light}.png are 1440×900 (1×) like the other
// settings screenshots, so no HiDPI display and no scaling step is needed.
//
// Not part of the suite (outside specs/). It brings the v15 test server up itself, so one command
// from a clean checkout is enough:
//   npm run e2e -- --spec e2e/walkthrough/screenshots-local-mutes.e2e.ts
import { browser } from "@wdio/globals";
import { app, testenv } from "../helpers/index.ts";
import { finishWizard } from "../helpers/chat-ui.ts";
import { openSettingsGroup } from "../helpers/settings-ui.ts";
import { addAndConnect } from "../helpers/voice-ui.ts";
import { both, scrollToText } from "./shots.ts";

describe("screenshots: Local mutes settings group (AC-15.8)", function () {
  this.timeout(300_000);

  before(async () => {
    await testenv.up("v15");
    await app.restart({ freshProfile: true });
    await browser.setWindowSize(1440, 900);
    await finishWizard("Mara");
    await addAndConnect();
  });

  it("shoots the group in dark and light", async () => {
    await both("15-local-mutes", async () => {
      await openSettingsGroup("Chat & Notifications");
      await scrollToText("Local mutes");
    });
  });
});
