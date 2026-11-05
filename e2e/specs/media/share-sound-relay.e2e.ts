// Spec 07 §4.10 (issue #435), the relay half of AC-7.61 and AC-7.70: a whole-screen share sent as
// a relay broadcast carries the same sound as the direct one (440 and 660 Hz, never Mumcord's own
// sounds or B's voice), and the relay sees no app name.
//
// Not automated: "the inspector shows encrypted audio frames as before" (the broadcast's audio
// frames are the existing FR-7.43 path, unit-tested in the engine); here the watch reports the
// broadcast path and the sound arrives through it.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts) or without PipeWire.
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-relay.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, apps, pwapps, relayenv } from "../../helpers/index.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import {
  P1_HZ,
  P2_HZ,
  cleanUpSound,
  expectHeard,
  expectSoundLine,
  share,
  startSharer,
  startViewer,
  stopShare,
  watch,
} from "../../helpers/share-sound-ui.ts";

const P1 = "Test App One";
const P2 = "Test App Two";

/** Settings → Privacy & Connections: "Relay only", so the share goes out as a broadcast. */
async function relayOnly(): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
  const radio = $("fieldset.p2p-modes input[type='radio'][value='relayOnly']");
  await radio.waitForExist();
  await radio.parentElement().click();
  await browser.waitUntil(async () => radio.isSelected(), { timeoutMsg: "relayOnly was not selected" });
  await browser.keys("Escape");
  await $("fieldset.p2p-modes").waitForExist({ reverse: true });
}

const run = relayenv.relayAvailable() && pwapps.pipewireRunning();

(run ? describe : describe.skip)("share sound through a relay broadcast (#435)", function () {
  this.timeout(900_000);
  this.bail(true);

  after(async () => {
    pwapps.stopTestApps();
    await apps.stopB();
    cleanUpSound();
    await relayenv.stopRelay();
  });

  it("AC-7.61: B hears the desktop sound through the broadcast", async () => {
    const { port } = await relayenv.startRelay("all");
    pwapps.startTestApp(P1, P1_HZ);
    pwapps.startTestApp(P2, P2_HZ);
    await startSharer({ sharePick: "screen" }, port);
    // The relay bot hands out relay tokens to registered users only.
    await registerOnServer();
    await relayOnly();
    await startViewer(port);
    await apps.onB(async () => {
      await registerOnServer();
      await relayOnly();
    });
    await share();
    await expectSoundLine("Sound: desktop");
    await watch();
    // The watch must run through the broadcast before its sound can be checked; the relay
    // tokens and the broadcast link take a moment after the view opens.
    await apps.onB(async () => {
      let path: string | undefined;
      await browser
        .waitUntil(
          async () => {
            const serverId = await app.invoke<number | null>("voice_server_get");
            const listed = await app.invoke<{ session: number }[]>("screen_share_list", { serverId });
            const stats = await app.invoke<{ path: string } | null>("screen_share_watch_stats", {
              serverId,
              session: listed[0]?.session,
            });
            path = stats?.path;
            return path === "broadcast";
          },
          { timeout: 45_000, interval: 500 },
        )
        .catch(() => {
          throw new Error(`B watches through ${String(path)}, want the broadcast`);
        });
    });
    await app.invoke("audio_test_sound");
    await expectHeard([P1_HZ, P2_HZ]);
  });

  it("AC-7.70: the relay sees no app name", async () => {
    const status = JSON.stringify(await relayenv.relayStatus());
    expect(status).not.toContain(P1);
    expect(status).not.toContain(P2);
    expect(status).not.toContain("testapp-");
    await stopShare();
  });
});
