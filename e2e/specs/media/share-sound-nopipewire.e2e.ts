// Spec 07 §4.10 (issue #435), AC-7.66: Linux without PipeWire (PulseAudio only) → the Sound switch
// is off and disabled with "No sound: needs PipeWire", in the start dialog and in Settings; the
// share works without sound; the stored setting is unchanged once PipeWire is back.
//
// The container normally runs PipeWire (MUMCORD_E2E_AUDIO=pipewire): this spec then stops it,
// starts PulseAudio in its place, and brings PipeWire back as container-run.sh started it. With
// MUMCORD_E2E_AUDIO=pulse it uses the PulseAudio the container started and skips the last part.
//
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-nopipewire.e2e.ts
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
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
const daemons: ChildProcess[] = [];

function quiet(cmd: string, args: string[]): void {
  try {
    execFileSync(cmd, args, { stdio: "ignore", timeout: 10_000 });
  } catch {
    // Not running: nothing to stop.
  }
}

async function waitFor(check: () => boolean, what: string): Promise<void> {
  await browser.waitUntil(() => Promise.resolve(check()), { timeout: 15_000, interval: 200, timeoutMsg: what });
}

/** Leaves only PulseAudio: stops PipeWire and WirePlumber, starts PulseAudio with a null sink. */
async function pulseOnly(): Promise<void> {
  quiet("pkill", ["-x", "wireplumber"]);
  quiet("pkill", ["-x", "pipewire"]);
  await waitFor(() => !pwapps.pipewireRunning(), "PipeWire did not stop");
  daemons.push(
    spawn(
      "pulseaudio",
      ["--daemonize=no", "--exit-idle-time=-1", "--load=module-null-sink sink_name=e2e-sink"],
      { stdio: "ignore" },
    ),
  );
}

/** Stops PulseAudio and starts PipeWire as container-run.sh does. */
async function pipewireBack(): Promise<void> {
  for (const d of daemons.splice(0)) d.kill();
  quiet("pkill", ["-x", "pulseaudio"]);
  spawn("pipewire", [], { stdio: "ignore", detached: true }).unref();
  await waitFor(() => pwapps.pipewireRunning(), "PipeWire did not come back");
  const runtime = process.env.XDG_RUNTIME_DIR ?? "";
  const conf = join(runtime, "wireplumber.conf");
  spawn("wireplumber", existsSync(conf) ? ["-c", conf] : [], { stdio: "ignore", detached: true }).unref();
  execFileSync("pw-cli", [
    "create-node",
    "adapter",
    "{ factory.name=support.null-audio-sink node.name=e2e-sink media.class=Audio/Sink object.linger=true audio.position=[FL FR] }",
  ], { stdio: "ignore", timeout: 10_000 });
}

describe("share sound without PipeWire (#435, AC-7.66)", function () {
  this.timeout(600_000);
  this.bail(true);

  after(async () => {
    await apps.stopB();
    cleanUpSound();
    if (hadPipeWire && !pwapps.pipewireRunning()) await pipewireBack();
  });

  it("the start dialog shows Sound off and disabled with the reason", async () => {
    if (hadPipeWire) await pulseOnly();
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
    await pipewireBack();
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
