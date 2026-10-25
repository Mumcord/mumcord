// AC-1.33 smoke (B32): the overlay window opens via its command, stays click-through on X11 (QA.61)
// and shows the talking Bot with the talking ring. C10 extends this to the full acceptance criteria
// (on top, click-through, restart).
import { join } from "node:path";
import { browser, expect, $ } from "@wdio/globals";
import { botInBackground, repoRoot, testenv, type BackgroundBot } from "../../helpers/index.ts";
import { run } from "../../helpers/run.ts";
import { addAndConnect, completeWizard, joinIfReadOnly, readTree } from "../../helpers/voice-ui.ts";

const speechPath = join(repoRoot, "testenv", "audio", "speech.wav");
const OVERLAY_TITLE = "Mumcord overlay";
const inputProbe = join(repoRoot, "e2e", "specs", "overlay", "x11-input.py");

interface X11Input {
  windows: number;
  inputRects: number;
  pointerGrabbed: boolean;
}

async function overlayInput(): Promise<X11Input> {
  const r = await run("python3", ["-I", inputProbe, OVERLAY_TITLE]);
  if (r.code !== 0) throw new Error(`x11-input failed: ${r.stderr}`);
  return JSON.parse(r.stdout) as X11Input;
}

async function invoke(command: string, args: object): Promise<void> {
  await browser.execute(
    async (c: string, a: object) => {
      const w = window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a: object) => Promise<unknown> };
      };
      await w.__TAURI_INTERNALS__.invoke(c, a);
    },
    command,
    args,
  );
}

/** Switches to the overlay window; returns its handle. */
async function switchToOverlay(main: string): Promise<string> {
  let found: string | undefined;
  await browser.waitUntil(
    async () => {
      for (const handle of await browser.getWindowHandles()) {
        if (handle === main) continue;
        await browser.switchToWindow(handle);
        if ((await browser.getTitle()) === OVERLAY_TITLE) {
          found = handle;
          return true;
        }
      }
      await browser.switchToWindow(main);
      return false;
    },
    { timeout: 15_000, timeoutMsg: "overlay window not found" },
  );
  if (found === undefined) throw new Error("overlay window not found");
  return found;
}

describe("overlay smoke", () => {
  let speaker: BackgroundBot | undefined;

  before(async () => {
    await testenv.reset("v15");
    await completeWizard("Tester");
    await addAndConnect();
    await joinIfReadOnly();
  });

  after(() => {
    speaker?.stop();
  });

  it("AC-1.33 opens via command and shows the talking Bot", async () => {
    const main = await browser.getWindowHandle();
    const hereC = (await readTree()).find((c) => c.people.includes("Tester"));
    const here = !hereC || hereC.level === 1 ? "/" : hereC.name;

    await invoke("overlay_set_enabled", { enabled: true });
    await switchToOverlay(main);

    // QA.61: click-through on X11 = an empty input shape (not even Tauri's 1×1 px corner) and
    // no pointer grab, so clicks reach every other window.
    let input: X11Input | undefined;
    await browser.waitUntil(
      async () => {
        input = await overlayInput();
        return input.windows === 1 && input.inputRects === 0;
      },
      { timeout: 10_000, timeoutMsg: "overlay still takes pointer input" },
    );
    expect(input?.pointerGrabbed).toBe(false);

    speaker = botInBackground(["script"], { name: "Bot", timeout: 60 });
    speaker.send(`join "${here}"`);
    for (let i = 0; i < 10; i++) speaker.send(`play ${speechPath}`);

    const talkingBot = $(
      "//li[contains(@class,'overlay-person')][@data-talking='true'][contains(., 'Bot')]",
    );
    await talkingBot.waitForExist({ timeout: 30_000, timeoutMsg: "overlay never showed Bot talking" });
    await $("//li[contains(., 'Bot')]//span[contains(@class,'avatar')][@data-talking='true']").waitForExist();

    await browser.switchToWindow(main);
    await invoke("overlay_set_enabled", { enabled: false });
    await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 1, {
      timeout: 10_000,
      timeoutMsg: "overlay window still open after disabling it",
    });
  });
});
