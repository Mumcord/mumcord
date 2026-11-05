// PipeWire test apps for the share-sound specs (spec 07 §4.10, AC-7.61 to 7.66, issue #435) and
// the measurement of what arrives on the receiving side. The container starts PipeWire with a
// null sink (see container-run.sh); outside it these helpers do not apply.
import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { RATE, readWav, writeWav } from "./voice-audio.ts";

/** The 1 kHz tone of Mumcord's own test notification: must never be in the share. */
export const OWN_SOUND_HZ = 1000;

export interface TestApp {
  /** `application.name` PipeWire shows; also the app hint for `hookEnv({ sharePick })`. */
  readonly name: string;
  readonly hz: number;
  stop(): void;
}

const apps = new Set<TestApp>();
let work: string | undefined;

function workDir(): string {
  work ??= mkdtempSync(join(tmpdir(), "mumcord-pwapps-"));
  return work;
}

/** Whether a PipeWire daemon answers, i.e. the container runs with `MUMCORD_E2E_AUDIO=pipewire`. */
export function pipewireRunning(): boolean {
  try {
    execFileSync("pw-cli", ["info", "0"], { stdio: "ignore", timeout: 5000 });
    return true;
  } catch {
    return false;
  }
}

/**
 * Starts an app that plays a sine of `hz` until stopped. Every app is its own copy of `pw-cat`
 * (binary `testapp-<slug>-play`), so apps differ by name and binary like real ones: a window
 * share follows one app's streams by process id and binary and must not pick up the others.
 */
export function startTestApp(name: string, hz: number): TestApp {
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-");
  const dir = workDir();
  const binary = join(dir, `testapp-${slug}-play`);
  const wav = join(dir, `${slug}-${String(hz)}.wav`);
  const sine = new Float32Array(RATE * 30);
  for (let i = 0; i < sine.length; i++) sine[i] = 0.5 * Math.sin((2 * Math.PI * hz * i) / RATE);
  writeWav(wav, sine);
  const pwCat = execFileSync("which", ["pw-cat"], { encoding: "utf8" }).trim();
  mkdirSync(dir, { recursive: true });
  copyFileSync(pwCat, binary);
  execFileSync("chmod", ["755", binary]);

  let child: ChildProcess | undefined;
  let stopped = false;
  // 30 s per run; start again when it ends so a long spec never goes silent.
  const play = (): void => {
    if (stopped) return;
    const started = Date.now();
    child = spawn(binary, ["--playback", "-P", `application.name=${name}`, wav], {
      stdio: "ignore",
    });
    child.on("exit", () => {
      // A player that dies at once (no daemon) must not turn into a busy loop.
      if (Date.now() - started > 1000) play();
    });
  };
  play();
  const app: TestApp = {
    name,
    hz,
    stop() {
      stopped = true;
      child?.kill();
      apps.delete(app);
    },
  };
  apps.add(app);
  return app;
}

/** Stops every test app and removes their files; call from `afterEach`/`after`. */
export function stopTestApps(): void {
  for (const app of [...apps]) app.stop();
  if (work) rmSync(work, { recursive: true, force: true });
  work = undefined;
}

/** Signal power of `x` at `hz` (Goertzel), normalised so a full-scale sine of that frequency is 1. */
export function tonePower(x: Float32Array, hz: number): number {
  const w = (2 * Math.PI * hz) / RATE;
  const coeff = 2 * Math.cos(w);
  let s1 = 0;
  let s2 = 0;
  for (const sample of x) {
    const s0 = sample + coeff * s1 - s2;
    s2 = s1;
    s1 = s0;
  }
  const power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
  return (4 * power) / (x.length * x.length);
}

/**
 * The frequencies (out of `candidates`) that carry sound in `x`: those whose power is at least
 * `minPower` (default: a sine of amplitude 0.05, well above what other tones leak in).
 */
export function dominantFrequencies(
  x: Float32Array,
  candidates: readonly number[],
  minPower = 0.05 ** 2,
): number[] {
  return candidates.filter((hz) => tonePower(x, hz) >= minPower);
}

/**
 * What the app played in the last `seconds` of its file output (the `out.wav` of `audio: "file:<in>:<out>"` of
 * the receiving app), reduced to the `candidates` that are audible: the receiving-side report for
 * "which app's sound arrives" (AC-7.61 to 7.64). A listed tone absent from the result is silent.
 */
export function receivedFrequencies(
  outputWav: string,
  candidates: readonly number[],
  seconds = 1,
): number[] {
  const all = readWav(outputWav);
  const tail = all.subarray(Math.max(0, all.length - Math.round(seconds * RATE)));
  return dominantFrequencies(tail, candidates);
}
