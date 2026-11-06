// Spec 07 §4.10 (issue #435), AC-7.66: Linux without PipeWire (PulseAudio only) → the Sound switch
// is off and disabled with "No sound: needs PipeWire", in the start dialog and in Settings; the
// share works without sound; the stored setting is unchanged once PipeWire is back.
//
// The container normally runs PipeWire (MUMCORD_E2E_AUDIO=pipewire): this spec then stops it,
// starts PulseAudio in its place, and brings PipeWire back as container-run.sh started it. With
// MUMCORD_E2E_AUDIO=pulse it uses the PulseAudio the container started and skips the last part.
//
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-nopipewire.e2e.ts
import { browser, $, expect } from "@wdio/globals";
import { app, apps, pwapps, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  cleanUpSound,
  confirmShareDialog,
  dialogSoundSwitch,
  openScreenSharingSettings,
  openShareDialog,
  soundSetting,
  soundSettingRow,
  expectHeard,
  expectSoundLine,
  shares,
  startSharer,
  startViewer,
  stopShare,
  watch,
} from "../../helpers/share-sound-ui.ts";

const NEEDS = "No sound: needs PipeWire";

const hadPipeWire = pwapps.pipewireRunning();
describe("share sound without PipeWire (#435, AC-7.66)", function () {
  this.timeout(600_000);
  this.bail(true);

  after(async () => {
    await apps.stopB();
    cleanUpSound();
    if (hadPipeWire && !pwapps.pipewireRunning()) await pwapps.pipewireBack();
  });

  it("the start dialog shows Sound off and disabled with the reason", async () => {
    if (hadPipeWire) await pwapps.pulseOnly();
    expect(pwapps.pipewireRunning()).toBe(false);
    await testenv.up("v15");
    await startSharer({ sharePick: "screen" }, ports.v15);
    await startViewer(ports.v15);
    await openShareDialog();
    const toggle = dialogSoundSwitch();
    expect(await toggle.isSelected()).toBe(false);
    expect(await toggle.isEnabled()).toBe(false);
    const note = $("//dialog//p[contains(@class,'share-start__soundnote')]");
    expect(await note.getText()).toBe(NEEDS);
    const described = (await toggle.getAttribute("aria-describedby")) ?? "";
    expect(described.split(" ")).toContain(await note.getAttribute("id"));
  });

  it("the share works without sound", async () => {
    await confirmShareDialog();
    await expectSoundLine("No sound");
    const own = (await shares()).at(0);
    expect(own?.soundReason).toBe("noPipeWire");
    await watch();
    await expectHeard([]);
    await stopShare();
  });

  it("Settings shows the same, and the stored setting stays on", async () => {
    await openScreenSharingSettings();
    const toggle = await soundSetting();
    expect(await toggle.isSelected()).toBe(false);
    expect(await toggle.isEnabled()).toBe(false);
    const status = (await soundSettingRow()).$("p.settings-switch__status");
    expect(await status.getText()).toBe(NEEDS);
    await browser.keys("Escape");
    const stored = await app.invoke<{ screenSound: boolean }>("settings_get");
    expect(stored.screenSound).toBe(true);
  });

  (hadPipeWire ? it : it.skip)("with PipeWire back, Sound is on again as stored", async () => {
    await pwapps.pipewireBack();
    await app.restart();
    const stored = await app.invoke<{ screenSound: boolean }>("settings_get");
    expect(stored.screenSound).toBe(true);
    await openScreenSharingSettings();
    const toggle = await soundSetting();
    await browser.waitUntil(async () => toggle.isEnabled(), { timeoutMsg: "the Sound setting stayed disabled" });
    expect(await toggle.isSelected()).toBe(true);
    await browser.keys("Escape");
  });
});
