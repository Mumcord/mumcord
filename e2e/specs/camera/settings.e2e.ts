// Spec 08, the camera's settings and its ways in and out besides the room: AC-8.19 (Settings →
// Camera values survive a restart; the next preview starts with the default background),
// AC-8.18 (screen lock), AC-8.20 (minimised window, tray badge, tooltip and "Turn off camera"),
// AC-8.21 (global hotkey) and AC-8.22 (camera unplugged, access denied).
//
// The screen lock comes from the debug `lockScreen` test command (`MUMCORD_TEST_SCREEN_LOCK=1`).
// Manual (real OS events, not in the X11 CI session): Windows WTS session lock, the macOS
// screen-lock notification and the logind lock signal on GNOME/KDE each turn the camera off with
// the setting on; the camera light goes off within 1 s (AC-8.25).
// The tray runs against a stand-in StatusNotifierWatcher (the CI session has no panel); the
// Windows and macOS trays are checked by hand.
//
// Skips itself without a mumpeer checkout (see relay-uses.e2e.ts).
// Run: npm run e2e -- --spec e2e/specs/camera/settings.e2e.ts
import type { ChildProcess } from "node:child_process";
import { $, browser, expect } from "@wdio/globals";
import { app, apps, relayenv } from "../../helpers/index.ts";
import { run, runOk } from "../../helpers/run.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { pickOption } from "../../helpers/servers-ui.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "../../helpers/voice-ui.ts";
import { startTrayHost } from "../app/flows.ts";
import {
  NAMES,
  cameraView,
  on,
  openCameraSettings,
  openPreview,
  previewDialog,
  setRoomOpen,
  settingsSwitch,
  sleep,
  startCamClient,
  tileOf,
  turnCameraOn,
  updateSettings,
  waitCameraState,
  waitText,
  waitTile,
} from "./flows.ts";

const HOTKEY = "F9";

// ---------------------------------------------------------------- tray (StatusNotifierItem)

async function gdbus(...args: string[]): Promise<string> {
  return (await runOk("gdbus", ["call", "--session", ...args])).stdout;
}

/** A's StatusNotifierItem bus name (ksni: `org.kde.StatusNotifierItem-<pid>-<n>`). */
async function trayName(): Promise<string> {
  const pid = app.appPid("a");
  let name: string | undefined;
  await browser.waitUntil(
    async () => {
      const names = await gdbus(
        "--dest", "org.freedesktop.DBus", "--object-path", "/org/freedesktop/DBus",
        "--method", "org.freedesktop.DBus.ListNames",
      );
      name = new RegExp(`org\\.kde\\.StatusNotifierItem-${String(pid)}-\\d+`).exec(names)?.[0];
      return name !== undefined;
    },
    { timeout: 20_000, timeoutMsg: "A's tray item never appeared on the bus" },
  );
  return name as string;
}

async function trayProperty(prop: "ToolTip" | "IconPixmap"): Promise<string> {
  return gdbus(
    "--dest", await trayName(), "--object-path", "/StatusNotifierItem",
    "--method", "org.freedesktop.DBus.Properties.Get", "org.kde.StatusNotifierItem", prop,
  );
}

/** Clicks the tray menu entry `label` through the item's dbusmenu. */
async function clickTrayEntry(label: string): Promise<void> {
  const dest = await trayName();
  let id: string | undefined;
  await browser.waitUntil(
    async () => {
      const layout = await gdbus(
        "--dest", dest, "--object-path", "/MenuBar",
        "--method", "com.canonical.dbusmenu.GetLayout", "0", "-1", "[]",
      );
      // Each entry: `(<id>, {'label': <'…'>, …}, [...])`.
      id = new RegExp(`\\((\\d+), \\{[^}]*'label': <'${label}'>`).exec(layout)?.[1];
      return id !== undefined;
    },
    { timeout: 10_000, timeoutMsg: `the tray menu has no "${label}"` },
  );
  await gdbus(
    "--dest", dest, "--object-path", "/MenuBar",
    "--method", "com.canonical.dbusmenu.Event", id as string, "clicked", "<int32 0>", "uint32 0",
  );
}

/** Minimises A's window like the user does (the window manager's minimise). */
async function minimiseA(): Promise<void> {
  const ids = (await run("xdotool", ["search", "--pid", String(app.appPid("a")), "--name", "^Mumcord$"])).stdout
    .trim()
    .split("\n")
    .filter(Boolean);
  if (ids.length === 0) throw new Error("A's window not found");
  for (const id of ids) await runOk("xdotool", ["windowminimize", "--sync", id]);
}

(relayenv.relayAvailable() ? describe : describe.skip)("camera settings, lock, tray, hotkey and device loss (AC-8.18 to 8.22)", function () {
  this.timeout(900_000);
  this.bail(true);

  let port = 0;
  let tray: ChildProcess | undefined;

  before(async () => {
    ({ port } = await relayenv.startRelay("all"));
    // The tray item registers with the watcher when A starts.
    tray = await startTrayHost();
  });

  after(async () => {
    tray?.kill();
    await apps.stopB();
    await apps.stopC();
    await relayenv.stopRelay();
  });

  it("AC-8.19: every Settings → Camera value survives a restart", async () => {
    await app.restart({
      freshProfile: true,
      env: { MUMCORD_TEST_CAMERA: "1", MUMCORD_TEST_SCREEN_LOCK: "1" },
    });
    await completeWizard(NAMES.a);
    await browser.setWindowSize(1400, 900);
    await openCameraSettings();
    // Every value away from its default.
    await pickOption("Send quality", "360p · 30 fps");
    await settingsSwitch("Mirror my self-view").click();
    await pickOption("Default background", "Blur");
    await $("//span[contains(@class,'settings-camera-seg')]//button[normalize-space()='Strong']").click();
    await pickOption("Effect quality", "Light (saves battery)");
    await settingsSwitch("Turn my camera off when my screen locks").click();
    await pickOption("Show cameras", "When I open the room");
    await settingsSwitch("Data saver").click();
    await settingsSwitch("Reduce motion").click();
    const changed = await app.invoke<Record<string, unknown>>("settings_get");
    expect(changed.cameraSendQuality).toBe("360p30");
    expect(changed.cameraDefaultBackground).toEqual({ kind: "blur" });

    await app.restart();
    const kept = await app.invoke<Record<string, unknown>>("settings_get");
    for (const key of [
      "cameraSendQuality",
      "cameraMirrorSelf",
      "cameraDefaultBackground",
      "cameraBlurStrength",
      "cameraEffectQuality",
      "cameraOffOnLock",
      "cameraShow",
      "cameraDataSaver",
      "cameraReduceMotion",
    ]) {
      expect({ [key]: kept[key] }).toEqual({ [key]: changed[key] });
    }
    // The page shows the kept values too.
    await openCameraSettings();
    expect(await settingsSwitch("Data saver").isSelected()).toBe(changed.cameraDataSaver);
    await browser.keys("Escape");

    await addAndConnect(`localhost:${String(port)}`);
    await joinIfReadOnly();
    await registerOnServer();
    // The next preview starts with the default background and blur strength.
    await openPreview();
    await previewDialog()
      .$("//label[contains(@class,'bg-mode')][@data-checked][contains(.,'Blur')]")
      .waitForExist({ timeout: 10_000 });
    expect(
      await previewDialog().$("//button[contains(@class,'cam-seg__button')][normalize-space()='Strong']").getAttribute("aria-pressed"),
    ).toBe("true");
    await previewDialog().$("button=Cancel").click();
    await waitCameraState("off");
  });

  it("AC-8.18: the screen lock turns the camera off only with the setting on", async () => {
    await startCamClient("b", port);
    await on("b", () => setRoomOpen(true));

    await updateSettings({ cameraOffOnLock: true });
    await turnCameraOn();
    await app.invoke("lock_screen");
    await waitCameraState("off", 3_000);
    await on("b", async () => {
      await browser.waitUntil(async () => (await tileOf(NAMES.a)) === undefined, {
        timeout: 15_000,
        timeoutMsg: "A's tile stayed after the lock",
      });
    });

    await updateSettings({ cameraOffOnLock: false });
    await turnCameraOn();
    await app.invoke("lock_screen");
    await sleep(3_000);
    expect((await cameraView())?.state).toBe("onAir");
  });

  it("AC-8.20: minimised, A keeps sending; the tray shows the badge and turns the camera off", async () => {
    const iconOn = await trayProperty("IconPixmap");
    expect(await trayProperty("ToolTip")).toContain("Camera on");
    await minimiseA();
    await on("b", async () => {
      await waitTile(NAMES.a);
      // Still playing well after the window went away.
      await sleep(5_000);
      expect((await tileOf(NAMES.a))?.state).toBe("playing");
    });
    expect((await cameraView())?.viewers.length).toBeGreaterThan(0);

    await clickTrayEntry("Turn off camera");
    await waitCameraState("off", 5_000);
    await browser.waitUntil(async () => !(await trayProperty("ToolTip")).includes("Camera on"), {
      timeout: 10_000,
      timeoutMsg: "the tooltip still says Camera on",
    });
    // The badge goes with the camera.
    expect(await trayProperty("IconPixmap")).not.toBe(iconOn);
    await runOk("xdotool", ["search", "--pid", String(app.appPid("a")), "--name", "^Mumcord$", "windowactivate"]);
  });

  it("AC-8.21: the hotkey opens the preview when off and turns the camera off when on", async () => {
    await openSettingsGroup("Voice & Audio");
    const field = $(
      "//div[contains(@class,'hotkey-row')][.//span[normalize-space(.)='Turn camera on / off']]//button[contains(@class,'hotkey-row__field')]",
    );
    await field.scrollIntoView();
    await field.click();
    await browser.keys(HOTKEY);
    await browser.waitUntil(async () => (await field.getText()).includes(HOTKEY), {
      timeout: 10_000,
      timeoutMsg: "camera hotkey not recorded",
    });
    await browser.keys("Escape");

    await runOk("xdotool", ["key", HOTKEY]);
    await previewDialog().waitForDisplayed({ timeout: 10_000 });
    expect((await cameraView())?.state).toBe("preview");
    // Nothing is sent from the preview.
    expect((await cameraView())?.broadcast ?? null).toBeNull();
    await on("b", async () => {
      await sleep(2_000);
      expect(await tileOf(NAMES.a)).toBeUndefined();
    });

    const confirm = previewDialog().$(".app-dialog__actions .button--primary");
    await confirm.waitForEnabled({ timeout: 20_000 });
    await confirm.click();
    await waitCameraState("onAir");
    await runOk("xdotool", ["key", HOTKEY]);
    await waitCameraState("off", 2_000);
  });

  it("AC-8.22: an unplugged camera stops with its notice", async () => {
    await startCamClient("c", port, { camera: "unplug_after=5s", name: "Carol" });
    await on("c", async () => {
      await turnCameraOn();
      const view = await waitCameraState("off", 20_000);
      expect(view.error).toBe("deviceGone");
      await waitText("Camera stopped: the camera was disconnected.");
    });
    await on("b", () => browser.waitUntil(async () => (await tileOf("Carol")) === undefined, {
      timeout: 15_000,
      timeoutMsg: "the unplugged camera's tile stayed",
    }));
    await apps.stopC();
  });

  it("AC-8.22: a refused camera shows the platform's text and button in the preview", async () => {
    // A new identity: "Carol" is registered to the previous profile's certificate.
    await startCamClient("c", port, { camera: "denied", name: "Dave" });
    await on("c", async () => {
      await openPreview();
      const status = previewDialog().$(".cam-dialog__status");
      await browser.waitUntil(async () => (await status.getText()).includes("Camera access was denied"), {
        timeout: 15_000,
        timeoutMsg: "the preview never said the access was denied",
      });
      // Linux: the portal can be asked again.
      await status.$("button=Try again").waitForDisplayed();
      expect(await previewDialog().$(".app-dialog__actions .button--primary").isEnabled()).toBe(false);
      await previewDialog().$("button=Cancel").click();
    });
  });
});
