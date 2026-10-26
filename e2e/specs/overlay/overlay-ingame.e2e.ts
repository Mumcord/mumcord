// In-game overlay on Linux (AC-1.34b): a test Vulkan program (crates/overlay-layer
// examples/headless_check, rendered with lavapipe to a headless surface) loads Mumcord's layer
// and must show the talking Bot in its frames. Checks the global On/Off switch, a per-game
// exception, the anti-cheat warning and that per-game options survive a restart. Exclusive
// fullscreen and the Windows Direct3D 11 hook need real hardware (manual checks in the spec).
import { existsSync } from "node:fs";
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { app, artifactsDir, bin, botInBackgroundOn, repoRoot, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { run } from "../../helpers/run.ts";
import { addAndConnect, completeWizard, joinIfReadOnly, readTree } from "../../helpers/voice-ui.ts";

const speechPath = join(repoRoot, "testenv", "audio", "speech.wav");
const GAME = "headless_check";

const manifest = () => join(app.profile(), "data", "vulkan", "implicit_layer.d", "mumcord_overlay.json");

let renders = 0;

/** Renders a few frames through the layer; exit 0 = the overlay's talking dot was drawn. The
 * last frame of each run lands in the artifacts for failure analysis. */
async function render(): Promise<boolean> {
  renders++;
  const r = await run(bin.headlessCheck, ["render"], {
    XDG_DATA_HOME: join(app.profile(), "data"),
    MUMCORD_CHECK_PPM: join(artifactsDir, `ingame-${String(renders)}.ppm`),
  });
  console.log(`[ingame] ${r.stdout.trim()} ${r.stderr.trim()}`);
  return r.code === 0;
}

/** The Bot's speech has pauses, so a frame may catch it silent: retry before calling it absent. */
async function expectDrawn(drawn: boolean): Promise<void> {
  let seen = false;
  for (let attempt = 0; attempt < 5 && !seen; attempt++) seen = await render();
  expect(seen).toBe(drawn);
}

async function openOverlaySettings(): Promise<void> {
  const button = $("button[aria-label='Quick settings']");
  await button.waitForClickable();
  await button.click();
  await $("//button[@role='menuitem'][contains(., 'All settings')]").click();
  await $("//button[@role='tab' and .='Overlay']").click();
}

async function closeSettings(): Promise<void> {
  await browser.keys("Escape");
  await $(".overlay__dialog").waitForExist({ reverse: true, timeout: 5000 });
}

function modeButton(label: "On" | "Off") {
  return $(`//div[@role='radiogroup'][@aria-label='In-game overlay']/button[.='${label}']`);
}

function gameRow() {
  return $(`//li[contains(@class,'overlay-settings__game')][.//label[.='${GAME}']]`);
}

/** A slider in the game's expanded options. */
function gameField(label: string) {
  return $(`//div[@role='group'][@aria-label='${GAME}']//div[label[.='${label}']]/input`);
}

/** The themed corner dropdown in the game's expanded options. */
function positionCombo() {
  return $(`//div[@role='group'][@aria-label='${GAME}']//div[label[.='Position']]//*[@role='combobox']`);
}

describe("in-game overlay (AC-1.34b)", () => {
  let speaker: BackgroundBot | undefined;

  before(async () => {
    await testenv.reset("v15");
    await completeWizard("Tester");
    await addAndConnect();
    await joinIfReadOnly();
    const ownC = (await readTree()).find((c) => c.people.includes("Tester"));
    speaker = botInBackgroundOn("v15", ["script"], { name: "Bot", timeout: 60 });
    speaker.send(`join "${!ownC || ownC.level === 1 ? "/" : ownC.name}"`);
    speaker.send(`play ${speechPath} --loop --seconds 300`);
  });

  after(() => {
    speaker?.stop();
  });

  it("AC-1.34b global On: the talking Bot is drawn into a program without a per-game entry", async () => {
    await openOverlaySettings();
    await modeButton("On").click();
    await expect(modeButton("On")).toHaveAttribute("aria-checked", "true");
    // The anti-cheat warning sits right below the switch.
    await expect($("//p[contains(., 'anti-cheat protection treat any overlay as cheating')]")).toBeDisplayed();
    await closeSettings();
    await browser.waitUntil(() => existsSync(manifest()), { timeout: 5000, timeoutMsg: "layer manifest not installed" });
    await expectDrawn(true);
  });

  it("AC-1.34b a program switched off per game shows nothing", async () => {
    await openOverlaySettings();
    const toggle = gameRow().$("input[role='switch']");
    await toggle.waitForExist({ timeout: 10_000, timeoutMsg: `${GAME} not offered in the games list` });
    await expect(toggle).toBeSelected();
    await toggle.click();
    await expect(toggle).not.toBeSelected();
    await closeSettings();
    await expectDrawn(false);
  });

  it("AC-1.34b global Off draws nothing anywhere", async () => {
    await openOverlaySettings();
    await gameRow().$(`button=Use default for ${GAME}`).click();
    await expect(gameRow().$("input[role='switch']")).toBeSelected();
    await modeButton("Off").click();
    await expect(modeButton("Off")).toHaveAttribute("aria-checked", "true");
    await closeSettings();
    await browser.waitUntil(() => !existsSync(manifest()), { timeout: 5000, timeoutMsg: "layer manifest still installed" });
    await expectDrawn(false);
  });

  it("AC-1.34b per-game position and opacity are kept after a restart", async () => {
    await openOverlaySettings();
    await gameRow().$(`button=Options for ${GAME}`).click();
    await positionCombo().click();
    await $("//*[@role='option' and normalize-space(.)='Bottom right']").click();
    const opacity = gameField("Opacity");
    const before = await opacity.getValue();
    await browser.execute((e: HTMLElement) => {
      e.focus();
    }, (await opacity.getElement()) as unknown as HTMLElement);
    // One step at a time: every change is saved and comes back as the new value.
    for (let i = 1; i <= 2; i++) {
      await browser.keys("ArrowLeft");
      await expect(opacity).toHaveValue(String(Number(before) - i * 5));
    }
    const changed = await opacity.getValue();
    await closeSettings();

    await app.restart();
    await openOverlaySettings();
    await gameRow().$(`button=Options for ${GAME}`).click();
    await browser.waitUntil(async () => (await positionCombo().getProperty("textContent")) === "Bottom right", {
      timeoutMsg: "position not kept after the restart",
    });
    await expect(gameField("Opacity")).toHaveValue(changed);
    await closeSettings();
  });
});
