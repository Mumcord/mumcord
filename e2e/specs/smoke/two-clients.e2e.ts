// Harness check for specs with two Mumcord clients (Epic 3.1, spec §10 "E2E with two clients"):
// instance B runs next to A with its own profile and identity, both see each other on the same
// server, and B keeps its identity across a restart while A keeps running.
import { expect } from "@wdio/globals";
import { apps, testenv } from "../../helpers/index.ts";
import { addAndConnect, completeWizard, personRow, reconnect } from "../../helpers/voice-ui.ts";

describe("harness: two Mumcord clients", () => {
  before(async () => {
    await testenv.up("v15");
  });

  it("A and B connect with their own identities and see each other", async () => {
    await completeWizard("Alice");
    await addAndConnect();
    await apps.startB();
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect();
      await personRow("Alice").waitForDisplayed({ timeout: 20_000 });
    });
    await personRow("Bob").waitForDisplayed({ timeout: 20_000 });
  });

  it("restarting B keeps its profile and leaves A connected", async () => {
    await apps.restartB();
    await apps.onB(async () => {
      // No wizard: the saved server and the identity survived the restart.
      await reconnect();
      await personRow("Alice").waitForDisplayed({ timeout: 20_000 });
    });
    await personRow("Bob").waitForDisplayed({ timeout: 20_000 });
    await expect(personRow("Alice")).toBeDisplayed();
  });
});
