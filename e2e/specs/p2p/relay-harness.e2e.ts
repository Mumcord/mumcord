// Smoke test of the relay E2E harness (spec 07, B4): the mumpeer profiles start, and clients A, B
// and C connect to the Mumble server of the environment and see each other. Runs only when a
// mumpeer checkout is available (MUMPEER_DIR, see helpers/relayenv.ts).
import { expect } from "@wdio/globals";
import { apps, relayenv } from "../../helpers/index.ts";
import { addAndConnect, completeWizard, personRow } from "../../helpers/voice-ui.ts";

const profiles: relayenv.RelayProfile[] = ["all", "no-files", "no-screen", "no-images", "no-broadcast"];
const oldImage = process.env.MUMCORD_E2E_OLD_MUMPEER_IMAGE;
const oldBotImage = process.env.MUMCORD_E2E_OLD_MUMBOT_IMAGE;

(relayenv.relayAvailable() ? describe : describe.skip)("relay harness (spec 07 B4)", function () {
  this.timeout(900_000);

  it("connects A, B and C to the profile 'all'", async () => {
    const relay = await relayenv.startRelay("all");
    const address = `localhost:${String(relay.port)}`;
    await completeWizard("Alice");
    await addAndConnect(address);
    await apps.startB({ env: relayenv.hookEnv({ fileCorrupt: true }) });
    await apps.onB(async () => {
      await completeWizard("Bob");
      await addAndConnect(address);
    });
    await apps.startC();
    await apps.onC(async () => {
      await completeWizard("Carol");
      await addAndConnect(address);
    });
    await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    await personRow("Carol").waitForDisplayed({ timeout: 30_000 });
    await apps.onC(async () => {
      await personRow("Alice").waitForDisplayed({ timeout: 30_000 });
      await personRow("Bob").waitForDisplayed({ timeout: 30_000 });
    });
  });

  for (const profile of profiles) {
    it(`starts the profile '${profile}'`, async () => {
      const relay = await relayenv.startRelay(profile);
      const status = await relayenv.relayStatus();
      console.log(`[harness] ${profile}: ${JSON.stringify(status)}`);
      expect(relay.profile).toBe(profile);
      if (profile === "no-broadcast") expect(status.broadcast).toBeUndefined();
    });
  }

  (oldImage ? it : it.skip)("starts the profile 'old'", async () => {
    await relayenv.startRelay("old", { image: oldImage, botImage: oldBotImage });
    console.log(`[harness] old: ${JSON.stringify(await relayenv.relayStatus())}`);
  });
});
