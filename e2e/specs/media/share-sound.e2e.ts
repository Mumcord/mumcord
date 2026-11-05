// Spec 07 §4.10 (issue #435): the sound of a whole-screen share on Linux with PipeWire.
//
//   AC-7.61  test apps P1 (440 Hz) and P2 (660 Hz) play, A's Mumcord plays its 1 kHz test sound
//            and B talks → A shares the whole screen; B gets 440 and 660 Hz, never 1 kHz or B's
//            own voice. The relay broadcast half is share-sound-relay.e2e.ts.
//   AC-7.64  during that share P3 (880 Hz) starts and P1 stops → B hears P3 within 1 s and no
//            longer P1; Mumcord's own sounds stay absent throughout.
//   AC-7.70  afterwards A's log (debug level) names no test app, binary or process id, and the
//            viewer's share state carries no app name.
//
// A's own playback goes to its null audio backend in the container, not to PipeWire (see
// helpers/share-sound-ui.ts), so the "never 1 kHz / B's voice" checks hold trivially here; they
// stay in the spec so a change in the test setup cannot silently start leaking them.
//
// Skips itself without PipeWire (MUMCORD_E2E_AUDIO=pulse or none).
// Run: npm run e2e -- --spec e2e/specs/media/share-sound.e2e.ts
import { expect } from "@wdio/globals";
import { app, apps, pwapps, testenv } from "../../helpers/index.ts";
import { ports } from "../../helpers/testenv.ts";
import {
  P1_HZ,
  PrivateNames,
  P2_HZ,
  P3_HZ,
  cleanUpSound,
  expectHeard,
  expectLogPrivate,
  expectSoundLine,
  msUntilHeard,
  share,
  shares,
  startSharer,
  startViewer,
  stopShare,
  watch,
} from "../../helpers/share-sound-ui.ts";

/** The capture adds a new stream within 1 s (AC-7.64); transport and B's file output add up to 0.5 s. */
const SWITCH_BUDGET_MS = 1500;

(pwapps.pipewireRunning() ? describe : describe.skip)("share sound, whole screen (#435)", function () {
  this.timeout(600_000);
  this.bail(true);

  const seen = new PrivateNames();
  const remember = (a: pwapps.TestApp): pwapps.TestApp => seen.add(a);

  after(async () => {
    pwapps.stopTestApps();
    await apps.stopB();
    cleanUpSound();
  });

  let p1: pwapps.TestApp;

  it("AC-7.61: B hears the desktop sound without Mumcord's own sounds", async () => {
    await testenv.up("v15");
    p1 = remember(pwapps.startTestApp("Test App One", P1_HZ));
    remember(pwapps.startTestApp("Test App Two", P2_HZ));
    await startSharer({ sharePick: "screen" }, ports.v15);
    await startViewer(ports.v15);
    await share();
    await expectSoundLine("Sound: desktop");
    await watch();
    // Mumcord's own test sound while B listens: it must not reach the share.
    await app.invoke("audio_test_sound");
    await expectHeard([P1_HZ, P2_HZ]);
    await app.invoke("audio_test_sound");
    await expectHeard([P1_HZ, P2_HZ], 2000);
  });

  it("AC-7.64: a stream that starts is added within 1 s, one that stops is gone", async () => {
    remember(pwapps.startTestApp("Test App Three", P3_HZ));
    const added = await msUntilHeard(P3_HZ, true);
    expect(added).toBeLessThanOrEqual(SWITCH_BUDGET_MS);
    p1.stop();
    const removed = await msUntilHeard(P1_HZ, false);
    expect(removed).toBeLessThanOrEqual(SWITCH_BUDGET_MS);
    await app.invoke("audio_test_sound");
    await expectHeard([P2_HZ, P3_HZ]);
  });

  it("AC-7.70: no app name reaches the viewer or the log", async () => {
    const atViewer = await apps.onB(shares);
    expect(atViewer.length).toBe(1);
    expect(atViewer[0]?.soundApp).toBeNull();
    seen.refresh();
    await stopShare();
    expectLogPrivate(seen.all());
  });
});
