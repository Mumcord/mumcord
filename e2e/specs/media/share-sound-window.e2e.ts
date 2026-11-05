// Spec 07 §4.10 (issue #435): the sound of a window share on Linux with PipeWire.
//
//   AC-7.62  the window's hint matches P1 clearly → no "Which app's sound?" list; B gets only
//            440 Hz. P3 starting during the share is not heard; P1 opening a second stream is.
//   AC-7.63  a window without a clear match → the list appears, lists P1 and P2 (not Mumcord) and
//            works by keyboard; choosing P2 → B gets only 660 Hz; "No sound" or Cancel → no share
//            sound while the picture goes on; the live bar says "Sound: {app}" or "No sound".
//   AC-7.70  after each part A's log (debug level) names no test app, binary or process id.
//
// The test backend has no window picker: the hook `sharePick` stands in for it, "window:<hint>"
// with the app hint the portal would give, "window" with none (README "Sound for the
// share-sound specs").
//
// Skips itself without PipeWire (MUMCORD_E2E_AUDIO=pulse or none).
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-window.e2e.ts
import { browser, $, $$, expect } from "@wdio/globals";
import { apps, pwapps, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  P1_HZ,
  PrivateNames,
  P1_SECOND_HZ,
  P2_HZ,
  P3_HZ,
  cleanUpSound,
  expectHeard,
  expectLogPrivate,
  expectSoundLine,
  share,
  shares,
  startSharer,
  startViewer,
  stopShare,
  watch,
} from "../../helpers/share-sound-ui.ts";

const P1 = "Test App One";
const P2 = "Test App Two";

const picker = () => $("dialog.sound-picker");
const listbox = () => picker().$("[role='listbox']");

/** The names of the rows in the open list, in order. */
async function rows(): Promise<string[]> {
  const options = await $$("dialog.sound-picker [role='option']");
  const names: string[] = [];
  for (const o of options) names.push((await o.getText()).trim());
  return names;
}

/** The name of the selected row (the listbox's active descendant). */
async function selectedRow(): Promise<string> {
  const id = await listbox().getAttribute("aria-activedescendant");
  return id === null ? "" : (await $(`[id='${id}']`).getText()).trim();
}

/** Waits for the list, then puts the keyboard focus on it. */
async function focusList(): Promise<void> {
  await picker().waitForDisplayed({ timeout: 20_000, timeoutMsg: "the sound list never opened" });
  await browser.waitUntil(async () => (await picker().$("h2").getText()) === "Which app's sound?");
  await browser.execute((el) => {
    el.focus();
  }, await listbox());
}

/** The share goes on (picture) and has no sound. */
async function expectSilentShare(): Promise<void> {
  await expectSoundLine("No sound");
  const own = (await shares()).at(0);
  expect(own?.soundChoice).toBeNull();
  await expect($(".sharer-bar")).toBeExisting();
  await watch();
  await expectHeard([]);
}

(pwapps.pipewireRunning() ? describe : describe.skip)("share sound, one window (#435)", function () {
  this.timeout(900_000);
  this.bail(true);

  const seen = new PrivateNames();
  const remember = (a: pwapps.TestApp): pwapps.TestApp => seen.add(a);

  after(async () => {
    pwapps.stopTestApps();
    await apps.stopB();
    cleanUpSound();
  });

  let p1: pwapps.TestApp;

  it("AC-7.62: a clear match shares only that app, with no list", async () => {
    await testenv.up("v15");
    p1 = remember(pwapps.startTestApp(P1, P1_HZ));
    remember(pwapps.startTestApp(P2, P2_HZ));
    await startSharer({ sharePick: `window:${P1}` }, ports.v15);
    await startViewer(ports.v15);
    await share();
    await expectSoundLine(`Sound: ${P1}`);
    expect(await picker().isExisting()).toBe(false);
    await watch();
    await expectHeard([P1_HZ]);
  });

  it("AC-7.62: a new app is not heard, a second stream of the shared app is", async () => {
    remember(pwapps.startTestApp("Test App Three", P3_HZ));
    await expectHeard([P1_HZ], 4000);
    p1.addStream(P1_SECOND_HZ);
    await expectHeard([P1_HZ, P1_SECOND_HZ]);
  });

  it("AC-7.70: the window share leaves no app name in the log", async () => {
    seen.refresh();
    await stopShare();
    expectLogPrivate(seen.all());
  });

  it("AC-7.63: without a clear match the list offers the apps, by keyboard", async () => {
    pwapps.stopTestApps();
    remember(pwapps.startTestApp(P1, P1_HZ));
    remember(pwapps.startTestApp(P2, P2_HZ));
    await startSharer({ sharePick: "window" }, ports.v15);
    await share();
    await focusList();
    expect(await rows()).toEqual([P1, P2, "No sound"]);
    expect((await rows()).some((r) => r.includes("Mumcord"))).toBe(false);
    const offered = (await shares()).at(0)?.soundChoice?.map((a) => a.name);
    expect(offered).toEqual([P1, P2]);
    expect(await selectedRow()).toBe(P1);
    await browser.keys("ArrowDown");
    expect(await selectedRow()).toBe(P2);
    await browser.keys("End");
    expect(await selectedRow()).toBe("No sound");
    await browser.keys("Home");
    await browser.keys("ArrowDown");
    expect(await selectedRow()).toBe(P2);
    await browser.keys("Enter");
    await picker().waitForExist({ reverse: true, timeout: 20_000, timeoutMsg: "the list stayed open" });
  });

  it("AC-7.63: choosing P2 shares only P2", async () => {
    await expectSoundLine(`Sound: ${P2}`);
    await watch();
    await expectHeard([P2_HZ]);
    await stopShare();
  });

  it("AC-7.63: \"No sound\" keeps the picture without sound", async () => {
    await share();
    await focusList();
    await browser.keys("End");
    await browser.keys("Enter");
    await picker().waitForExist({ reverse: true, timeout: 20_000, timeoutMsg: "the list stayed open" });
    await expectSilentShare();
    await stopShare();
  });

  it("AC-7.63: Cancel keeps the picture without sound", async () => {
    await share();
    await focusList();
    await browser.keys("Escape");
    await picker().waitForExist({ reverse: true, timeout: 20_000, timeoutMsg: "the list stayed open" });
    await expectSilentShare();
  });

  it("AC-7.70: the list and the choices leave no app name in the log", async () => {
    seen.refresh();
    await stopShare();
    expectLogPrivate(seen.all());
  });
});
