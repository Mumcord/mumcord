// Camera sharing (spec 08) in the running client: launch environment, the camera entry points,
// the preview dialog, the camera room and pixel sampling of the preview and the tiles. Shared by
// the specs in specs/camera (part 1: avail, onair, effects, room-limits; part 2 builds on it).
//
// Pixels: the test camera (`MUMCORD_TEST_CAMERA=1`, crates/camera/src/testcam.rs) draws a person
// (head around (640+sway, 250), shoulders from y 380 down) in front of eight 160 px colour bars,
// 1280×720, with a sway of at most ±24 px. The sample points below are fractions of the picture,
// so they fit the 640×360 preview and every tile size. Person points sit well inside the outline
// for every sway; background points sit 3–4 px next to a bar edge, where a blur mixes two colours
// and a replaced background changes completely.
import { browser, $ } from "@wdio/globals";
import * as app from "./app.ts";
import * as apps from "./apps.ts";
import { registerOnServer } from "./files-ui.ts";
import { openSettingsGroup } from "./settings-ui.ts";
import { addAndConnect, completeWizard, personRow } from "./voice-ui.ts";

/** App environment with the virtual test camera. */
export const CAMERA_ENV: Record<string, string> = { MUMCORD_TEST_CAMERA: "1" };

/** App environment with the test camera and its known person outline as segmentation mask. */
export const MASK_ENV: Record<string, string> = {
  ...CAMERA_ENV,
  MUMCORD_TEST_SEGMENT_MASK: "fixture",
};

/** The texts of the camera availability reasons (camera.json `unavailable.*`). */
export const REASON = {
  noRelay: "This server has no relay, so camera sharing isn't available.",
  relayTooOld: "This server's relay is too old for camera sharing.",
  relayDisallows: "This server's relay doesn't allow camera sharing.",
  needsRelayMode: "Camera sharing needs the relay. Change it in Privacy & Connections.",
} as const;

export const ROOM_FULL = "The camera room is full (9 of 9 cameras).";

// ---------------------------------------------------------------------------------------------
// Connecting

/** Joins the shown server's channel when its read-only notice offers it. */
export async function joinVoice(): Promise<void> {
  const join = $(".userpanel-readonly__join");
  if (!(await join.isDisplayed())) return;
  await join.click();
  await $(".userpanel-readonly").waitForExist({ reverse: true, timeout: 15_000 });
}

/**
 * The running client (fresh profile) creates `name`, connects to the relay environment's server
 * on `port`, joins the channel and registers: the relay bot gives relay tokens to registered users
 * only, and cameras always go through the relay.
 */
export async function connectRegistered(name: string, port: number): Promise<void> {
  await completeWizard(name);
  await addAndConnect(`localhost:${String(port)}`);
  await joinVoice();
  await registerOnServer();
}

/** Restarts A on a fresh profile with `env` and connects it as `name` (registered). */
export async function startA(name: string, port: number, env = CAMERA_ENV): Promise<void> {
  await app.restart({ freshProfile: true, env });
  await connectRegistered(name, port);
}

/** Starts B on a fresh profile with `env` and connects it as `name` (registered). */
export async function startB(name: string, port: number, env = CAMERA_ENV): Promise<void> {
  await apps.startB({ env });
  await apps.onB(() => connectRegistered(name, port));
}

/** Starts C on a fresh profile with `env` and connects it as `name` (registered). */
export async function startC(name: string, port: number, env = CAMERA_ENV): Promise<void> {
  await apps.startC({ env });
  await apps.onC(() => connectRegistered(name, port));
}

/** Settings → Privacy & Connections: picks the peer-to-peer mode of the running client. */
export async function setP2pMode(mode: "directRelay" | "relayOnly" | "directOnly"): Promise<void> {
  await openSettingsGroup("Privacy & Connections");
  const radio = $(`fieldset.p2p-modes input[type='radio'][value='${mode}']`);
  await radio.waitForExist();
  await radio.parentElement().click();
  await browser.waitUntil(async () => radio.isSelected(), {
    timeoutMsg: `${mode} was not selected`,
  });
  await browser.keys("Escape");
  await $("fieldset.p2p-modes").waitForExist({ reverse: true });
}

/** The "Relay allows" row of the shown server's info panel (opens and closes the panel). */
export async function relayAllows(): Promise<string> {
  await $("button[aria-label^='Server options']").click();
  await $("//*[@role='menuitem'][normalize-space()='Server info']").click();
  const row = $("//dt[normalize-space()='Relay allows']/following-sibling::dd[1]");
  try {
    await row.waitForExist({ timeout: 30_000, timeoutMsg: "server info has no Relay allows row" });
    return (await row.getText()).trim();
  } finally {
    await browser.keys("Escape");
  }
}

/** Blocks `name` from the person menu in the tree, confirming the dialog. */
export async function blockPerson(name: string): Promise<void> {
  const row = personRow(name);
  await row.waitForDisplayed({ timeout: 30_000 });
  await row.click({ button: "right" });
  const item = $("//*[@role='menuitem'][normalize-space()='Block']");
  await item.waitForClickable();
  await item.click();
  await $(`//dialog[.//h2[normalize-space()='Block ${name}?']]`).waitForDisplayed();
  await $("[data-testid='block-confirm']").click();
  await $(`//dialog[.//h2[normalize-space()='Block ${name}?']]`).waitForExist({ reverse: true });
}

// ---------------------------------------------------------------------------------------------
// Engine state (Tauri commands, see the G-1 contract)

export interface CameraView {
  serverId: number;
  state: "off" | "preview" | "requesting" | "onAir";
  /** A `CameraFailure` code ("roomFull", "relayDown", …). */
  error: string | null;
  /** The broadcast id while on air; `null` for a camera that runs without a relay broadcast. */
  broadcast: string | null;
  /** The people who receive the camera now, with the path each of them gets (FR-8.94). */
  viewers: CameraViewer[];
}

/** How one viewer gets the own camera (FR-8.62, FR-8.94). */
export interface CameraViewer {
  session: number;
  path: "relay" | "direct";
}

/** The own camera of the running client (`camera_view`), `null` when it is off. */
export async function cameraView(): Promise<CameraView | null> {
  return app.invoke<CameraView | null>("camera_view");
}

/** Waits until the own camera is in `state` (`"off"` also matches no view at all). */
export async function waitCameraState(state: CameraView["state"], timeout = 20_000): Promise<void> {
  let seen: CameraView | null = null;
  await browser.waitUntil(
    async () => {
      seen = await cameraView();
      return (seen?.state ?? "off") === state;
    },
    {
      timeout,
      interval: 250,
      timeoutMsg: `camera never reached ${state}: ${JSON.stringify(seen)}`,
    },
  );
}

// ---------------------------------------------------------------------------------------------
// Entry points: user-panel button and Apps-menu entry

/** The camera button in the user panel. */
export function cameraButton() {
  return $("button.camera-button");
}

/** The reason on the user-panel camera button, `null` when the camera can be turned on. */
async function buttonReason(): Promise<string | null> {
  const button = cameraButton();
  if (!(await button.isExisting())) return "<no camera button>";
  if ((await button.getAttribute("aria-disabled")) !== "true") return null;
  return button.getAttribute("data-tooltip");
}

/** The camera entry of the composer's Apps menu (the menu must be open). */
export function cameraMenuItem() {
  return $(".chat-apps__menu .camera-menu-item");
}

/** Opens the composer's Apps menu. */
export async function openAppsMenu(): Promise<void> {
  const button = $("button.chat-apps__button");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  await cameraMenuItem().waitForDisplayed();
}

/**
 * Waits until both entry points say `reason` (disabled with that text as tooltip and menu line),
 * or are enabled when `reason` is null. Availability can take a while after connecting (relay
 * announcement, registration), hence the long wait.
 */
export async function expectCameraBlock(reason: string | null, timeout = 60_000): Promise<void> {
  let seen: string | null = "";
  try {
    await browser.waitUntil(async () => (seen = await buttonReason()) === reason, {
      timeout,
      interval: 500,
    });
  } catch {
    throw new Error(`camera button: want ${JSON.stringify(reason)}, got ${JSON.stringify(seen)}`);
  }
  await openAppsMenu();
  try {
    const item = cameraMenuItem();
    const read = async (): Promise<string | null> =>
      (await item.getAttribute("aria-disabled")) === "true"
        ? (await item.$(".camera-menu-item__hint").getText()).trim()
        : null;
    await browser.waitUntil(async () => (seen = await read()) === reason, {
      timeout: 10_000,
      timeoutMsg: `Apps-menu camera entry: want ${JSON.stringify(reason)}, got ${JSON.stringify(seen)}`,
    });
  } finally {
    await browser.keys("Escape");
  }
}

// ---------------------------------------------------------------------------------------------
// Preview dialog

export function previewDialog() {
  return $("dialog.cam-dialog");
}

export const PREVIEW_CANVAS = "dialog.cam-dialog canvas.cam-preview__canvas";

/** Opens the preview with the user-panel camera button and waits for the first picture. */
export async function openPreview(): Promise<void> {
  const button = cameraButton();
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  await previewDialog().waitForDisplayed({
    timeout: 15_000,
    timeoutMsg: "no camera preview dialog",
  });
  await waitPicture(PREVIEW_CANVAS);
}

/** Presses Cancel in the preview and waits until the dialog is gone. */
export async function cancelPreview(): Promise<void> {
  await previewDialog().$("button=Cancel").click();
  await previewDialog().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Presses "Turn on camera" in the preview and waits until the camera is on air. */
export async function turnOnFromPreview(timeout = 30_000): Promise<void> {
  const confirm = previewDialog().$("button.button--primary");
  await confirm.waitForEnabled({ timeout: 15_000, timeoutMsg: "Turn on camera stays disabled" });
  await confirm.click();
  await waitCameraState("onAir", timeout);
  await previewDialog().waitForDisplayed({ reverse: true, timeout: 10_000 });
}

/** Opens the preview and turns the camera on with the background chosen by `choose`. */
export async function turnOn(choose?: () => Promise<void>): Promise<void> {
  await openPreview();
  if (choose) await choose();
  await turnOnFromPreview();
}

/** Turns the own camera off with the user-panel button. */
export async function turnOff(): Promise<void> {
  const button = cameraButton();
  await browser.waitUntil(async () => (await button.getAttribute("aria-pressed")) === "true", {
    timeoutMsg: "the camera button does not show the camera as on",
  });
  await button.click();
  await waitCameraState("off");
}

/** Picks a background mode in the open preview. */
export async function pickBackgroundMode(mode: "None" | "Blur" | "Image"): Promise<void> {
  const label = previewDialog().$(
    `.//div[contains(@class,'bg-modes')]//label[normalize-space()='${mode}']`,
  );
  await label.click();
  await browser.waitUntil(async () => (await label.getAttribute("data-checked")) !== null, {
    timeoutMsg: `background mode ${mode} was not selected`,
  });
}

/** Picks Light or Strong blur in the open preview (Blur must be the mode). */
export async function pickBlurStrength(strength: "Light" | "Strong"): Promise<void> {
  const button = previewDialog().$(
    `.//button[contains(@class,'cam-seg__button')][normalize-space()='${strength}']`,
  );
  await button.click();
  await browser.waitUntil(async () => (await button.getAttribute("aria-pressed")) === "true");
}

/** Picks the background image with this accessible name ("Warm light waves", "Your image 1"). */
export async function pickImage(name: string): Promise<void> {
  const input = previewDialog().$(
    `.//label[contains(@class,'bg-thumb')]/input[@aria-label='${name}']`,
  );
  await input.waitForExist({ timeout: 15_000, timeoutMsg: `no background image "${name}"` });
  await input.parentElement().click();
  await browser.waitUntil(async () => input.isSelected(), {
    timeoutMsg: `background image "${name}" was not selected`,
  });
}

/** Waits until every built-in background tile shows its decoded thumbnail. */
export async function waitBuiltinThumbnails(count: number): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(
        (expected) =>
          [
            ...document.querySelectorAll<HTMLImageElement>("dialog.cam-dialog .bg-thumb img"),
          ].filter((img) => img.complete && img.naturalWidth > 0).length >= expected,
        count,
      ),
    {
      timeout: 3_000,
      timeoutMsg: `the ${String(count)} background thumbnails did not show within 3 s`,
    },
  );
}

/** The "Background effects are not available …" line of the open preview, `null` without one. */
export async function effectsUnavailable(): Promise<string | null> {
  // Matches the load line itself; `normalize-space()` on `*` would also match its ancestors.
  const line = previewDialog().$(
    ".//p[contains(@class,'cam-load')][contains(normalize-space(),'Background effects are not available')]",
  );
  return (await line.isExisting()) ? (await line.getText()).trim() : null;
}

// ---------------------------------------------------------------------------------------------
// Camera room

/** Opens the camera room from the mini bar when it is not shown yet. */
export async function openRoom(): Promise<void> {
  const room = $(".cam-room__head");
  const open = $(
    "//div[contains(@class,'mini-bar')][starts-with(@aria-label,'Camera room')]//button[contains(@class,'mini-bar__button')]",
  );
  await browser.waitUntil(async () => (await room.isDisplayed()) || (await open.isDisplayed()), {
    timeout: 20_000,
    timeoutMsg: "neither the camera room nor its mini bar shows",
  });
  if (!(await room.isDisplayed())) {
    await open.click();
    await room.waitForDisplayed({ timeout: 10_000 });
  }
}

/** The tile of `name`'s camera ("Alice, camera on"). */
export function tile(name: string) {
  return $(`div.cam-tile[aria-label^='${name}, camera on']`);
}

/** CSS selector of the video canvas in `name`'s tile, for `samplePixels`. */
export function tileCanvas(name: string): string {
  return `div.cam-tile[aria-label^='${name}, camera on'] canvas.cam-video__canvas`;
}

/** Waits until `name`'s tile shows and its canvas has a picture. */
export async function waitTilePicture(name: string, timeout = 30_000): Promise<void> {
  await tile(name).waitForDisplayed({ timeout, timeoutMsg: `no camera tile for ${name}` });
  await waitPicture(tileCanvas(name), timeout);
}

/** Whether the channel tree shows a camera icon in `name`'s row (the own one is red). */
export async function hasTreeCamera(name: string): Promise<boolean> {
  return personRow(name).$(".camera-tree-icon").isExisting();
}

/** Whether `name`'s tree row has the own (red) camera icon. */
export async function hasOwnTreeCamera(name: string): Promise<boolean> {
  return personRow(name).$(".camera-tree-icon[data-self]").isExisting();
}

/** The grid of the camera room: number of columns (`--cols`) and tiles. */
export async function gridShape(): Promise<{ cols: number; tiles: number } | null> {
  return browser.execute(() => {
    const grid = document.querySelector<HTMLElement>(".cam-grid");
    if (!grid) return null;
    return {
      cols: Number(grid.style.getPropertyValue("--cols")),
      tiles: grid.querySelectorAll(".cam-grid__item").length,
    };
  });
}

// ---------------------------------------------------------------------------------------------
// Screen-reader announcements

/** Starts recording what the ARIA live regions say (read with `announcements`). */
export async function recordAnnouncements(): Promise<void> {
  await browser.execute(() => {
    const w = window as unknown as { __camLive?: string[]; __camLiveObs?: MutationObserver };
    w.__camLive = [];
    w.__camLiveObs?.disconnect();
    const observer = new MutationObserver(() => {
      for (const region of document.querySelectorAll("[aria-live]")) {
        const text = region.textContent.trim();
        if (text !== "" && w.__camLive?.at(-1) !== text) w.__camLive?.push(text);
      }
    });
    observer.observe(document.body, { subtree: true, childList: true, characterData: true });
    w.__camLiveObs = observer;
  });
}

/** What the live regions said since `recordAnnouncements`. */
export async function announcements(): Promise<string[]> {
  return browser.execute(() => (window as unknown as { __camLive?: string[] }).__camLive ?? []);
}

// ---------------------------------------------------------------------------------------------
// Pixels

export type Point = readonly [x: number, y: number];
export type Rgb = [r: number, g: number, b: number];

const SRC_W = 1280;
const SRC_H = 720;
const at = (x: number, y: number): Point => [x / SRC_W, y / SRC_H];

/** Inside the person for every sway: head and shoulders. */
export const PERSON_POINTS: Point[] = [
  at(640, 220),
  at(640, 280),
  at(560, 620),
  at(720, 620),
  at(640, 680),
];

/** Background next to bar edges: above the head and beside the shoulders. */
export const BACKGROUND_POINTS: Point[] = [
  ...[1, 2, 3, 4, 5, 6, 7].flatMap((k) => [at(160 * k - 4, 60), at(160 * k + 3, 60)]),
  at(156, 690),
  at(163, 690),
  at(316, 690),
  at(323, 690),
  at(956, 690),
  at(963, 690),
  at(1116, 690),
  at(1123, 690),
];

/**
 * The colours at `points` (fractions of the picture) of the 2-D canvas `selector`, each the
 * average of a 3×3 patch; `null` while the canvas is missing or has no picture yet.
 */
export async function samplePixels(selector: string, points: Point[]): Promise<Rgb[] | null> {
  // Flat coordinates: the WebDriver typings of `execute` mangle tuple arguments.
  const flat = points.flatMap(([x, y]) => [x, y]);
  const result: number[][] | null = await browser.execute(
    (sel: string, coords: number[]) => {
      const canvas = document.querySelector<HTMLCanvasElement>(sel);
      if (!canvas || canvas.width < 4 || canvas.height < 4) return null;
      const context = canvas.getContext("2d");
      if (!context) return null;
      const out: number[][] = [];
      for (let p = 0; p < coords.length; p += 2) {
        const [fx, fy] = [coords[p], coords[p + 1]];
        const x = Math.min(canvas.width - 3, Math.max(0, Math.round(fx * canvas.width) - 1));
        const y = Math.min(canvas.height - 3, Math.max(0, Math.round(fy * canvas.height) - 1));
        const data = context.getImageData(x, y, 3, 3).data;
        const sum = [0, 0, 0];
        for (let i = 0; i < data.length; i += 4) {
          sum[0] += data[i];
          sum[1] += data[i + 1];
          sum[2] += data[i + 2];
        }
        out.push(sum.map((v) => Math.round(v / 9)));
      }
      return out;
    },
    selector,
    flat,
  );
  return result?.map((rgb) => [rgb[0], rgb[1], rgb[2]]) ?? null;
}

/** Waits until the canvas shows a picture: the person's head is not black. */
export async function waitPicture(selector: string, timeout = 30_000): Promise<void> {
  await browser.waitUntil(
    async () => {
      const head = await samplePixels(selector, [PERSON_POINTS[0]]);
      return head !== null && head[0].some((v) => v > 40);
    },
    { timeout, interval: 500, timeoutMsg: `no picture in ${selector}` },
  );
}

/** Mean absolute difference per colour channel between two samples of the same points. */
export function meanDiff(a: Rgb[], b: Rgb[]): number {
  let total = 0;
  a.forEach((p, i) => {
    total += Math.abs(p[0] - b[i][0]) + Math.abs(p[1] - b[i][1]) + Math.abs(p[2] - b[i][2]);
  });
  return total / (a.length * 3);
}

/** Person and background samples of a canvas, averaged over `frames` reads 300 ms apart. */
export async function sampleRegions(
  selector: string,
  frames = 3,
): Promise<{ person: Rgb[]; background: Rgb[] }> {
  const reads: { person: Rgb[]; background: Rgb[] }[] = [];
  for (let i = 0; i < frames; i++) {
    const person = await samplePixels(selector, PERSON_POINTS);
    const background = await samplePixels(selector, BACKGROUND_POINTS);
    if (!person || !background) throw new Error(`no picture in ${selector}`);
    reads.push({ person, background });
    await browser.pause(300);
  }
  const avg = (pick: (r: (typeof reads)[number]) => Rgb[]): Rgb[] =>
    pick(reads[0]).map(
      (_, i) =>
        [0, 1, 2].map((c) =>
          Math.round(reads.reduce((s, r) => s + pick(r)[i][c], 0) / reads.length),
        ) as Rgb,
    );
  return { person: avg((r) => r.person), background: avg((r) => r.background) };
}

/**
 * Waits until the background of `selector` differs from `baseline` by more than `minBackground`
 * while the person stays within `maxPerson` (mean channel difference). Returns the last sample.
 */
export async function waitBackgroundChanged(
  selector: string,
  baseline: { person: Rgb[]; background: Rgb[] },
  opts: {
    minBackground?: number;
    maxPerson?: number;
    timeout?: number;
    /** The background must also come within `max` of this solid colour (an image fixture). */
    near?: { colour: Rgb; max: number };
  } = {},
): Promise<{ person: Rgb[]; background: Rgb[] }> {
  const { minBackground = 12, maxPerson = 25, timeout = 20_000, near } = opts;
  let last = baseline;
  let diffs = { background: 0, person: 0, near: 0 };
  try {
    await browser.waitUntil(
      async () => {
        last = await sampleRegions(selector, 2);
        diffs = {
          background: meanDiff(last.background, baseline.background),
          person: meanDiff(last.person, baseline.person),
          near: near
            ? meanDiff(
                last.background,
                last.background.map(() => near.colour),
              )
            : 0,
        };
        return (
          diffs.background > minBackground &&
          diffs.person <= maxPerson &&
          (near === undefined || diffs.near < near.max)
        );
      },
      { timeout, interval: 500 },
    );
  } catch {
    throw new Error(
      `${selector}: background diff ${diffs.background.toFixed(1)} (want > ${String(minBackground)}), person diff ${diffs.person.toFixed(1)} (want ≤ ${String(maxPerson)})${near ? `, distance to the image colour ${diffs.near.toFixed(1)} (want < ${String(near.max)})` : ""}`,
    );
  }
  return last;
}
