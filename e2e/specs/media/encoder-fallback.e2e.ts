// Spec 14 §11 (hwcodec crate): encoding, hardware encoder detection and the software AV1 self-test
// come from the external `hwcodec` crate. What the user sees must be what it was before:
//
//   AC-14.9   with the debug hook that makes every hardware encoder fail, a share between two
//             clients starts on a software encoder and Settings shows the same status text as
//             before: AV1 on the processor when the self-test passes, VP9 when it fails.
//   AC-14.11  Settings' "Check again" re-runs detection and the software AV1 self-test and shows
//             the results: once for real (no hook, so the log tells that both ran again) and once
//             with a forced self-test result that the status then shows.
//
// The expected texts are the ones of `ui/src/locales/en/settings.json` (`screenShare.*`) and of
// `ScreenSharingSection.test.tsx`, not read from the running app.
//
// The probe hook replaces detection altogether, so the cells of AC-14.9 cannot show that
// detection runs; the first cell of AC-14.11 runs without it for that reason. What it finds
// depends on the machine (no graphics card in the container), so that cell only accepts the
// texts the status can show and does not name one.
//
// The share itself is the test backend's pattern source (the app is built with `test-backend`, see
// e2e/container-run.sh), so no real screen is picked; `sharePick: "screen"` is the test hook.
//
// Run: npm run e2e -- --spec e2e/specs/media/encoder-fallback.e2e.ts
import { $, browser, expect } from "@wdio/globals";
import { app, apps, relayenv, testenv } from "../../helpers/index.ts";
import { hookEnv, type TestHooks } from "../../helpers/relayenv.ts";
import {
  openScreenSharingSettings,
  share,
  sharerLog,
  startSharer,
  startViewer,
  watch,
} from "../../helpers/share-sound-ui.ts";
import { reconnect } from "../../helpers/voice-ui.ts";
import { on, serverId } from "../camera/flows.ts";
import { waitPath } from "./flows.ts";

/** Every hardware encoder is found but fails its smoke test (AC-14.9). */
const NO_HARDWARE = ["av1_hw:smoke_fail", "vp9_hw:smoke_fail"];

/** The encoder the own share runs on, as `screen_share_quality` reports it. */
interface ActiveEncoder {
  codec: string;
  gpu: string | null;
}

/** The log lines that tell detection and the self-test ran (message text only, no field values). */
const PROBE_LINE = "probing the screen share encoders";
const SELFTEST_LINES = ["AV1 self-test measured", "AV1 self-test did not run"];
/** A processor without AVX2 fails the self-test without running it, so it logs this instead. */
const NO_AVX2_LINE = "software AV1 unavailable";

/** The status line above the encoder list on the open Screen sharing page. */
const statusLine = () =>
  $("//section[.//ul[@aria-label='Encoder status']]//p[@role='status']");

const checkAgainButton = () =>
  $("//section[.//ul[@aria-label='Encoder status']]//button[normalize-space()='Check again']");

/** The value of one row of the encoder list, by the row's label. */
async function row(label: string): Promise<string> {
  const value = $(
    `//ul[@aria-label='Encoder status']/li[span[1][normalize-space()='${label}']]/span[2]`,
  );
  await value.waitForExist({ timeout: 15_000, timeoutMsg: `no encoder row "${label}"` });
  return (await value.getText()).trim();
}

/** Waits until the status line reads `text`; detection and the self-test take a moment. */
async function waitStatus(text: string, timeout = 60_000): Promise<void> {
  let seen = "";
  await browser
    .waitUntil(
      async () => {
        seen = (await statusLine().getText()).trim();
        return seen === text;
      },
      { timeout, interval: 500 },
    )
    .catch(() => {
      throw new Error(`the encoder status reads "${seen}", want "${text}"`);
    });
}

/** Waits until the check has ended and returns the status line. */
async function waitChecked(timeout = 180_000): Promise<string> {
  let seen = "";
  await browser
    .waitUntil(
      async () => {
        seen = (await statusLine().getText()).trim();
        return seen.startsWith("Active encoder: ");
      },
      { timeout, interval: 500 },
    )
    .catch(() => {
      throw new Error(`the encoder check never ended, the status reads "${seen}"`);
    });
  return seen;
}

/** How often `text` is in A's log. */
function logCount(text: string): number {
  return sharerLog().split(text).length - 1;
}

function selftestLogCount(): number {
  return SELFTEST_LINES.reduce((sum, line) => sum + logCount(line), 0);
}

/** Waits until the own share has opened its encoder and returns it. */
async function shareEncoder(): Promise<ActiveEncoder> {
  const started = Date.now();
  for (;;) {
    const quality = await app.invoke<{ encoder: ActiveEncoder | null } | null>(
      "screen_share_quality",
      { serverId: await serverId() },
    );
    const encoder = quality?.encoder ?? null;
    if (encoder !== null) return encoder;
    if (Date.now() - started > 45_000) throw new Error("A's share never opened an encoder");
    await browser.pause(500);
  }
}

/** Restarts A on its profile with other hooks and brings it back into the channel. */
async function restartSharer(hooks: TestHooks): Promise<void> {
  await app.restart({ env: hookEnv(hooks) });
  await reconnect();
  await browser.setWindowSize(1400, 900);
}

/**
 * Records every text the status line takes and whether "Check again" was disabled meanwhile:
 * a forced or fast check shows "Checking…" too briefly to poll for.
 */
async function recordStatus(): Promise<void> {
  await browser.execute(() => {
    const list = document.querySelector("ul[aria-label='Encoder status']");
    const card = list?.closest("section");
    if (!card) throw new Error("no encoder status card");
    const seen = { texts: [] as string[], disabled: false };
    (window as unknown as { __encoderSeen: typeof seen }).__encoderSeen = seen;
    const note = () => {
      const text = card.querySelector("p[role='status']")?.textContent.trim() ?? "";
      if (seen.texts.at(-1) !== text) seen.texts.push(text);
      for (const button of card.querySelectorAll("button")) {
        if (button.textContent.trim() === "Check again" && button.disabled) seen.disabled = true;
      }
    };
    note();
    new MutationObserver(note).observe(card, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
    });
  });
}

function recordedStatus(): Promise<{ texts: string[]; disabled: boolean }> {
  return browser.execute(
    () =>
      (window as unknown as { __encoderSeen: { texts: string[]; disabled: boolean } })
        .__encoderSeen,
  );
}

describe("encoder fallback and check again with the hwcodec crate (AC-14.9, 14.11)", function () {
  this.timeout(900_000);
  this.bail(true);

  before(async () => {
    // The plain test server: no relay and no bot, so the watch is a direct one.
    await relayenv.stopRelay();
    await testenv.up("v15");
  });

  after(async () => {
    await apps.stopB();
  });

  it("AC-14.9: with every hardware encoder failing the share runs AV1 on the processor", async () => {
    const port = testenv.ports.v15;
    await startSharer({ encoderProbe: NO_HARDWARE, svtSelftest: "pass", sharePick: "screen" }, port);
    await startViewer(port);
    await share();
    await watch();
    await on("b", () => waitPath("direct"));

    expect(await shareEncoder()).toEqual({ codec: "av1", gpu: null });

    await openScreenSharingSettings();
    await waitStatus("Active encoder: AV1 (processor)");
    expect(await row("AV1 encode")).toBe("Not available");
    expect(await row("AV1 encode on the processor")).toBe("Software, keeps up to 1080p30");
    expect(await row("VP9 encode")).toBe("Software");
  });

  it("AC-14.9: with a failed self-test as well the share runs VP9 in software", async () => {
    await restartSharer({ encoderProbe: NO_HARDWARE, svtSelftest: "fail", sharePick: "screen" });
    await share();
    await watch();
    await on("b", () => waitPath("direct"));

    expect(await shareEncoder()).toEqual({ codec: "vp9", gpu: null });

    await openScreenSharingSettings();
    await waitStatus("Active encoder: VP9 (AV1 too heavy for this PC)");
    expect(await row("AV1 encode")).toBe("Not available");
    expect(await row("AV1 encode on the processor")).toBe("Not available");
    expect(await row("VP9 encode")).toBe("Software");
  });

  it("AC-14.11: Check again runs detection and the self-test again and shows the result", async () => {
    // No encoder hook: detection and the self-test are the real ones.
    await restartSharer({ sharePick: "screen" });
    await openScreenSharingSettings();
    await waitChecked();
    const probes = logCount(PROBE_LINE);
    const selftests = selftestLogCount();

    await recordStatus();
    await checkAgainButton().click();
    // The log is written a moment after the event; detection comes first, then the self-tests.
    await browser.waitUntil(() => logCount(PROBE_LINE) > probes, {
      timeout: 60_000,
      timeoutMsg: "Check again did not run detection again (no new probe line in the log)",
    });
    const status = await waitChecked();

    const seen = await recordedStatus();
    expect(seen.texts).toContain("Checking…");
    expect(seen.disabled).toBe(true);
    expect(seen.texts.at(-1)).toBe(status);
    expect(await checkAgainButton().isEnabled()).toBe(true);

    if (logCount(NO_AVX2_LINE) === 0) {
      expect(selftestLogCount()).toBeGreaterThan(selftests);
    }
    // What the machine can do is not the test's to say, only that the result is one of the texts.
    expect(status).toMatch(
      /^Active encoder: (AV1 \(processor\)|AV1 \(.+ card\)|VP9 \(software\)|VP9 \(.+ card\)|VP9 \(AV1 too heavy for this PC\)|VP9 \(your picture is above what AV1 manages here\))$/i,
    );
    expect(await row("AV1 encode on the processor")).toMatch(
      /^(Not available|Software, keeps up to \d+p\d+)$/,
    );
    expect(await row("AV1 encode")).toMatch(/^(Not available|.*[Gg]raphics card)$/);
    expect(await row("VP9 encode")).toMatch(/^(Software|.*[Gg]raphics card)$/);
  });

  it("AC-14.11: Check again shows a failed self-test as AV1 too heavy", async () => {
    // Detection stays the real one; only the self-test result is forced.
    await restartSharer({ svtSelftest: "fail", sharePick: "screen" });
    await openScreenSharingSettings();
    await waitChecked();
    const probes = logCount(PROBE_LINE);

    await checkAgainButton().click();
    // With the self-test forced the check is over within a moment, too fast for "Checking…" to
    // be on screen; the new probe line in the log is what tells that it ran.
    await browser.waitUntil(() => logCount(PROBE_LINE) > probes, {
      timeout: 60_000,
      timeoutMsg: "Check again did not run detection again (no new probe line in the log)",
    });
    await waitChecked();

    expect(await row("AV1 encode on the processor")).toBe("Not available");
    // Without a graphics card (the container) the forced failure leaves VP9 in software.
    if ((await row("AV1 encode")) === "Not available") {
      await waitStatus("Active encoder: VP9 (AV1 too heavy for this PC)");
    }
  });
});
