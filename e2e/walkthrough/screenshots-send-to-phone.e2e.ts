// Screenshots of "Send to your phone" (spec 05, FR-5.64; mockup 23 of mumcord-mobile#14), taken
// from the running client: the selection step and the codes, each in dark and light. One
// identity and four saved servers, like the mockup. No server is contacted.
//
// A runs on a display of its own at 2x scale, so the pictures come out at 2880×1800 like the
// other documentation shots.
//
// Not part of the suite (outside specs/):
//   npm run e2e -- --spec e2e/walkthrough/screenshots-send-to-phone.e2e.ts
import type { ChildProcess } from "node:child_process";
import { browser } from "@wdio/globals";
import { app } from "../helpers/index.ts";
import { finishWizard } from "../helpers/chat-ui.ts";
import {
  dialogButtonIn,
  openSettingsSection,
  settingsAction,
  settingsDialog,
  settingsTitle,
} from "../helpers/servers-ui.ts";
import { both, HIDPI_ENV, startHidpiDisplay } from "./shots.ts";

const TITLE = "Send to your phone";
const SERVERS = ["Home", "Guild night", "Work", "Friends"];

async function openDialog(): Promise<void> {
  await openSettingsSection("account");
  await settingsAction("Send to your phone…").click();
  await settingsTitle(TITLE).waitForDisplayed();
}

describe("screenshots: send to your phone", function () {
  this.timeout(300_000);
  let xvfb: ChildProcess | undefined;

  before(async () => {
    xvfb = await startHidpiDisplay();
    await app.restart({ freshProfile: true, env: { ...HIDPI_ENV } });
    await browser.setWindowSize(1440, 900);
    await finishWizard("Lea");
    for (const [index, label] of SERVERS.entries()) {
      await app.invoke("server_add", {
        input: {
          label,
          address: `voice-${String(index + 1)}.example.org`,
          port: 64738,
          username: "Lea",
          password: { kind: "keep" },
        },
      });
    }
    // The server count of the dialog comes from the list the window loaded at start.
    await app.restart({ env: { ...HIDPI_ENV } });
    await browser.setWindowSize(1440, 900);
  });

  after(() => {
    xvfb?.kill();
  });

  it("the selection step", async () => {
    await both("settings-send-to-phone-select", openDialog);
  });

  it("the codes", async () => {
    await both("settings-send-to-phone-codes", async () => {
      await openDialog();
      await dialogButtonIn(TITLE, "Show codes").click();
      await settingsDialog(TITLE)
        .$('[data-testid="phone-transfer-qr"]')
        .waitForDisplayed({ timeout: 30_000 });
    });
  });
});
