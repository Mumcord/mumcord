// Spec 07 §4.10 (issue #435), AC-7.65: with the Sound switch off (in the share start dialog and,
// separately, as the Settings default) no share sound is sent, although a test app plays and the
// picked whole screen would carry it; the Settings default is remembered across restarts.
//
// "No audio frames are sent" is checked where it shows: B plays none of the app's tone and its
// watch reports no share sound level, and A's own share has its sound off.
//
// Skips itself without PipeWire (MUMCORD_E2E_AUDIO=pulse or none): without it there is no sound
// to hold back (that case is share-sound-nopipewire.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-off.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, apps, pwapps, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import { reconnectSaved, startScreenShare } from "../../helpers/chat-ui.ts";
import {
  P1_HZ,
  cleanUpSound,
  confirmShareDialog,
  dialogSoundSwitch,
  openScreenSharingSettings,
  openShareDialog,
  soundSetting,
  expectHeard,
  expectSoundLine,
  shares,
  startSharer,
  startViewer,
  stopShare,
  watch,
} from "../../helpers/share-sound-ui.ts";

/** B watches A's share: no tone arrives and the watch reports no share sound. */
async function expectNoShareSound(): Promise<void> {
  await expectSoundLine("No sound");
  const own = (await shares()).at(0);
  expect(own?.sound).toBe(false);
  await watch();
  await expectHeard([], 5000);
  const level = await apps.onB(async () => {
    const serverId = await app.invoke<number | null>("voice_server_get");
    const listed = await app.invoke<{ session: number }[]>("screen_share_list", { serverId });
    const session = listed[0]?.session;
    return app.invoke<{ rms: number } | null>("screen_share_watch_stats", { serverId, session });
  });
  expect(level?.rms ?? 0).toBeLessThan(0.001);
}

(pwapps.pipewireRunning() ? describe : describe.skip)("share sound switch off (#435, AC-7.65)", function () {
  this.timeout(600_000);
  this.bail(true);

  after(async () => {
    pwapps.stopTestApps();
    await apps.stopB();
    cleanUpSound();
  });

  it("the switch on: the share carries the app's sound (control)", async () => {
    await testenv.up("v15");
    pwapps.startTestApp("Test App One", P1_HZ);
    await startSharer({ sharePick: "screen" }, ports.v15);
    await startViewer(ports.v15);
    await startScreenShare();
    await expectSoundLine("Sound: desktop");
    await watch();
    await expectHeard([P1_HZ]);
    await stopShare();
  });

  it("the switch off in the start dialog: no share sound", async () => {
    await openShareDialog();
    const toggle = dialogSoundSwitch();
    await toggle.waitForDisplayed({ timeout: 20_000, timeoutMsg: "the start dialog has no Sound switch" });
    expect(await toggle.isSelected()).toBe(true);
    await toggle.click();
    expect(await toggle.isSelected()).toBe(false);
    await confirmShareDialog();
    await expectNoShareSound();
    await stopShare();
  });

  it("the switch off as the Settings default: no share sound", async () => {
    await openScreenSharingSettings();
    const toggle = await soundSetting();
    await toggle.waitForExist();
    expect(await toggle.isSelected()).toBe(true);
    await toggle.click();
    await browser.waitUntil(async () => !(await toggle.isSelected()), { timeoutMsg: "the Sound setting stayed on" });
    await browser.keys("Escape");
    await $(".settings__nav").waitForExist({ reverse: true });
    await openShareDialog();
    const inDialog = dialogSoundSwitch();
    await inDialog.waitForDisplayed({ timeout: 20_000 });
    expect(await inDialog.isSelected()).toBe(false);
    await confirmShareDialog();
    await expectNoShareSound();
    await stopShare();
  });

  it("the Settings default stays off after a restart", async () => {
    await app.restart();
    await reconnectSaved();
    const stored = await app.invoke<{ screenSound: boolean }>("settings_get");
    expect(stored.screenSound).toBe(false);
    await openScreenSharingSettings();
    expect(await (await soundSetting()).isSelected()).toBe(false);
    await browser.keys("Escape");
  });
});
