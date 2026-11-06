// Spec 07 §4.10 (issue #435), AC-7.71: the share-sound parts look like their mockups
// (docs/mockups/epic-4.1-screenshare on the mockups branch: 08 "Which app's sound?", 09 the start
// dialog's sound line, 10 the live bar's sound line), in dark and light.
//
// The repo has no image-diff helper, so this spec asserts the structural parts of each mockup
// (texts, the outlined icons, the mockup's square colours, the live bar's buttons on one row also
// at the narrowest chat width with the members panel open) and saves one screenshot per part and
// theme to e2e/artifacts/mockup-shots/, which CI uploads for the side-by-side check by eye.
//
// The test backend has no window picker: the hook `sharePick` stands in for it ("window" with no
// app hint, so the list appears; README "Sound for the share-sound specs").
//
// Skips itself without PipeWire (MUMCORD_E2E_AUDIO=pulse or none).
// Run: npm run e2e -- --spec e2e/specs/media/share-sound-looks.e2e.ts
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { browser, $, $$, expect } from "@wdio/globals";
import type { ChainablePromiseElement } from "webdriverio";
import { pwapps, testenv } from "../../helpers/index.ts";
import { artifactsDir } from "../../helpers/paths.ts";
import { openSettingsGroup } from "../../helpers/settings-ui.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  P1_HZ,
  P2_HZ,
  cleanUpSound,
  confirmShareDialog,
  dialogSoundSwitch,
  expectSoundLine,
  openShareDialog,
  startSharer,
  stopShare,
} from "../../helpers/share-sound-ui.ts";

const P1 = "Test App One";
const P2 = "Test App Two";

/** The fallback square colours of mockup 08, as the browser reports them. */
const MOCKUP_SQUARES = [
  "rgb(230, 96, 0)",
  "rgb(29, 185, 84)",
  "rgb(255, 136, 0)",
  "rgb(229, 57, 53)",
];

const shots = join(artifactsDir, "mockup-shots");
const shot = (name: string) => browser.saveScreenshot(join(shots, name));

const picker = () => $("dialog.sound-picker");

/** Switches A to `theme` in Settings, as a person would, and waits until the page shows it. */
async function useTheme(theme: "dark" | "light"): Promise<void> {
  // Straight to Appearance: a search only lists its hits and does not open the page by itself.
  await openSettingsGroup("Appearance");
  const mode = $("//label[normalize-space(.)='Dark or light']/following::*[@role='combobox'][1]");
  await mode.waitForClickable({ timeout: 10_000 });
  await mode.click();
  const label = theme === "dark" ? "Dark" : "Light";
  await $(`//*[@role='option'][normalize-space()='${label}']`).click();
  await browser.waitUntil(
    async () => (await browser.execute(() => document.documentElement.dataset.theme)) === theme,
    { timeout: 10_000, timeoutMsg: `the ${theme} theme was never applied` },
  );
  await browser.keys(["Escape"]);
  await $(".settings__nav").waitForExist({ reverse: true, timeout: 10_000 });
}

/**
 * True when the icon is drawn as an outline (stroke, no fill), as in the mockups. Reads the
 * computed style of the drawn shapes: the app sets fill and stroke on an inner group, the mockup
 * on the <svg> itself, and both draw the same outline.
 */
async function outlined(svg: ChainablePromiseElement): Promise<boolean> {
  return browser.execute((el) => {
    const shapes = [...el.querySelectorAll("path, rect, circle, line, polyline, polygon, ellipse")];
    return (
      shapes.length > 0 &&
      shapes.every((shape) => {
        const style = getComputedStyle(shape);
        return style.fill === "none" && style.stroke !== "none" && style.strokeWidth !== "0px";
      })
    );
  }, await svg);
}

/** The vertical centres of the live bar's visible buttons. */
async function buttonCentres(): Promise<number[]> {
  return browser.execute(() =>
    [...document.querySelectorAll(".sharer-bar__actions > *")]
      .filter((el) => el instanceof HTMLElement && el.offsetParent !== null)
      .map((el) => {
        const box = el.getBoundingClientRect();
        return box.top + box.height / 2;
      }),
  );
}

/** Opens the members panel when it is closed. */
async function showMembers(): Promise<void> {
  const show = $("//button[starts-with(@aria-label,'Show members')]");
  if (await show.isExisting()) await show.click();
  await $("//button[starts-with(@aria-label,'Hide members')]").waitForExist({ timeout: 10_000 });
}

(pwapps.pipewireRunning() ? describe : describe.skip)("share sound looks like its mockups (#435)", function () {
  this.timeout(600_000);
  this.bail(true);

  before(async () => {
    mkdirSync(shots, { recursive: true });
    await testenv.up("v15");
    pwapps.startTestApp(P1, P1_HZ);
    pwapps.startTestApp(P2, P2_HZ);
    await startSharer({ sharePick: "window" }, ports.v15);
  });

  after(() => {
    pwapps.stopTestApps();
    cleanUpSound();
  });

  for (const theme of ["dark", "light"] as const) {
    it(`AC-7.71 (${theme}): the start dialog's sound line (mockup 09)`, async () => {
      await browser.setWindowSize(1440, 900);
      await useTheme(theme);
      await openShareDialog();
      await expect(dialogSoundSwitch()).toBeExisting();
      expect(await dialogSoundSwitch().isSelected()).toBe(true);
      await expect($("//dialog[@open]")).toHaveText(
        expect.stringContaining(
          "Whole screen: desktop sound without Mumcord. One window: only that app's sound.",
        ),
      );
      await shot(`09-share-start-sound-on-${theme}.png`);
    });

    it(`AC-7.71 (${theme}): "Which app's sound?" (mockup 08)`, async () => {
      await confirmShareDialog();
      await picker().waitForDisplayed({ timeout: 20_000, timeoutMsg: "the sound list never opened" });
      await expect(picker().$("h2")).toHaveText("Which app's sound?");
      const rows = await $$("dialog.sound-picker [role='option']");
      const names: string[] = [];
      for (const row of rows) names.push((await row.getText()).trim());
      expect(names).toEqual([P1, P2, "No sound"]);
      // Apps without an icon of their own get a square in one of the mockup's colours.
      for (const name of [P1, P2]) {
        const square = await picker().$(
          `//*[@role='option'][normalize-space()='${name}']//span[contains(@class,'sound-picker__icon')]`,
        );
        const colour = await browser.execute((el) => getComputedStyle(el).backgroundColor, square);
        expect(MOCKUP_SQUARES).toContain(colour);
      }
      expect(await outlined(picker().$(".sound-picker__icon--none svg"))).toBe(true);
      await expect(picker().$("//button[normalize-space()='Share sound']")).toBeExisting();
      await expect(picker().$("//button[normalize-space()='Cancel']")).toBeExisting();
      await shot(`08-app-picker-${theme}.png`);
    });

    it(`AC-7.71 (${theme}): the live bar's sound line (mockup 10)`, async () => {
      // The first row is selected: Enter shares P1's sound.
      await browser.execute((el) => {
        el.focus();
      }, await picker().$("[role='listbox']"));
      await browser.keys("Enter");
      await picker().waitForExist({ reverse: true, timeout: 20_000, timeoutMsg: "the list stayed open" });
      await expectSoundLine(`Sound: ${P1}`);
      const source = $(".sharer-bar__source");
      expect((await source.getText()).startsWith("Window")).toBe(true);
      const badge = source.$(
        ".//span[contains(concat(' ',@class,' '),' share-badge ')][starts-with(normalize-space(),'Sound:')]",
      );
      expect(await outlined(badge.$("svg"))).toBe(true);
      const centres = await buttonCentres();
      expect(centres.length).toBeGreaterThanOrEqual(6);
      expect(Math.max(...centres) - Math.min(...centres)).toBeLessThan(4);
      await shot(`10-live-bar-sound-app-${theme}.png`);
    });

    it(`AC-7.71 (${theme}): the live bar keeps its buttons on one row when narrow`, async () => {
      // 800 px is the app's minimum width (members panel over the chat); 1000 px is the narrowest
      // window with the members panel beside the chat, which leaves the chat at its narrowest.
      for (const width of [800, 1000]) {
        await browser.setWindowSize(width, 700);
        await showMembers();
        const centres = await buttonCentres();
        expect(centres.length).toBeGreaterThanOrEqual(6);
        expect(Math.max(...centres) - Math.min(...centres)).toBeLessThan(4);
        // Icons only, but every button keeps its name for screen readers.
        for (const name of ["Pause", "Sound", "Change window", "Stop sharing"]) {
          await expect(
            $(`//div[contains(@class,'sharer-bar')]//button[normalize-space()='${name}' or @aria-label='${name}']`),
          ).toBeExisting();
        }
        await shot(`10-live-bar-narrow-${String(width)}-${theme}.png`);
      }
      await browser.setWindowSize(1440, 900);
      await stopShare();
    });
  }
});
