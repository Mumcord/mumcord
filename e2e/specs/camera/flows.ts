// Shared steps of the camera E2E part 2 (spec 08, AC-8.9 … AC-8.27): clients A, B and C with the
// virtual test camera on the mumpeer test environment, the camera state as the engine reports it,
// and a hand-made Mumble client for the fuzzing of `mumcord.camera`.
//
// The relay bot hands out relay tokens to registered users only, so every client registers on the
// server (`registerOnServer`), which also makes the next `startRelay` wipe the server first.
import { connect as tlsConnect, type TLSSocket } from "node:tls";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { $, browser } from "@wdio/globals";
import { app, apps } from "../../helpers/index.ts";
import { waitForAbsent } from "../../helpers/dom.ts";
import { registerOnServer } from "../../helpers/files-ui.ts";
import { openAllSettings } from "../../helpers/settings-ui.ts";
import { addAndConnect, completeWizard, joinIfReadOnly } from "../../helpers/voice-ui.ts";
import type { AppId } from "../../helpers/app.ts";

export const NAMES: Record<AppId, string> = { a: "Alice", b: "Bob", c: "Carol" };

/** `MUMCORD_TEST_CAMERA` values of the virtual camera (spec 08 §10.3). */
export type TestCamera = "1" | "absent" | "denied" | `unplug_after=${string}`;

export interface CamClientOptions {
  /** The virtual camera (default `"1"`, one working camera). */
  camera?: TestCamera;
  /** More app environment, e.g. `relayenv.hookEnv({ keepOldKey: true })`. */
  env?: Record<string, string>;
  /** Debug log level, so the camera's debug lines reach the log file (AC-8.26). */
  debugLog?: boolean;
  /**
   * Name instead of `NAMES[id]`. A registered name belongs to its certificate, so a client
   * started again with a fresh profile needs a new one.
   */
  name?: string;
  /** Audio backend, e.g. `file:<in.wav>:<out.wav>` so the client talks (see `LaunchOptions`). */
  audio?: string;
}

export const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/** Runs `fn` on client `id` (A is the default WebDriver session). */
export function on<T>(id: AppId, fn: () => Promise<T>): Promise<T> {
  if (id === "b") return apps.onB(fn);
  if (id === "c") return apps.onC(fn);
  return fn();
}

/**
 * Starts client `id` with a fresh profile and the test camera, connects it to the relay
 * environment's server on `port` and registers it there.
 */
export async function startCamClient(
  id: AppId,
  port: number,
  opts: CamClientOptions = {},
): Promise<void> {
  const env = { MUMCORD_TEST_CAMERA: opts.camera ?? "1", ...opts.env };
  const launch = { env, ...(opts.audio === undefined ? {} : { audio: opts.audio }) };
  if (id === "a") await app.restart({ freshProfile: true, ...launch });
  else if (id === "b") await apps.startB(launch);
  else await apps.startC(launch);
  if (opts.debugLog) {
    // The log level applies from the next start.
    await on(id, async () => {
      const settings = await app.invoke<Record<string, unknown>>("settings_get");
      await app.invoke("settings_update", { settings: { ...settings, logLevel: "debug" } });
    });
    if (id === "a") await app.restart();
    else if (id === "b") await apps.restartB();
    else await apps.restartC();
  }
  await on(id, async () => {
    await completeWizard(opts.name ?? NAMES[id]);
    // The direct camera path asks before it opens. These specs are not about that question;
    // leaving it unanswered makes the viewer's tile fail while the sender is still waiting.
    await updateSettings({ allowDirectConnections: "yes" });
    await addAndConnect(`localhost:${String(port)}`);
    await joinIfReadOnly();
    await registerOnServer();
    // The room, the strip and the tile badges need a wide enough window.
    await browser.setWindowSize(1400, 900);
  });
}

// ---------------------------------------------------------------- engine state

export type CameraState = "off" | "preview" | "requesting" | "onAir";

export interface CameraView {
  serverId: number;
  state: CameraState;
  error: string | null;
  broadcast: string | null;
  viewers: CameraViewer[];
}

/** How one viewer gets the own camera (FR-8.62, FR-8.94). */
export interface CameraViewer {
  session: number;
  path: "relay" | "direct";
}

/** Waits until the own camera lists `session` as a viewer and returns the viewer. */
export async function waitOwnViewer(session: number, timeout = 20_000): Promise<CameraViewer> {
  let viewers: CameraViewer[] = [];
  await browser
    .waitUntil(
      async () => {
        viewers = (await cameraView())?.viewers ?? [];
        return viewers.some((v) => v.session === session);
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`the own viewers are ${JSON.stringify(viewers)}; want ${String(session)}`);
    });
  return viewers.find((v) => v.session === session) as CameraViewer;
}

export type TileState = "announced" | "joining" | "playing" | "full" | "failed" | "hidden";

export interface CameraTile {
  sharer: number;
  state: TileState;
  hidden: boolean;
  quality: "auto" | "dataSaver" | "best";
  dataSaver: boolean;
  fpsBadge: number | null;
}

export interface CameraRoomView {
  serverId: number;
  open: boolean;
  videosHidden: boolean;
  tiles: CameraTile[];
}

/** The voice server of the current client. */
export async function serverId(): Promise<number> {
  const id = await app.invoke<number | null>("voice_server_get");
  if (id === null) throw new Error("no voice server");
  return id;
}

/**
 * The own camera as the engine reports it; `null` while it is off. The "Camera stopped" reason of
 * an off camera only goes to the UI (its notice), never to this view.
 */
export function cameraView(): Promise<CameraView | null> {
  return app.invoke<CameraView | null>("camera_view");
}

/** The camera room of the current client's voice server. */
export async function cameraRoom(): Promise<CameraRoomView> {
  return app.invoke<CameraRoomView>("camera_room", { serverId: await serverId() });
}

interface Snapshot {
  selfSession: number;
  users: { session: number; name: string; channelId: number }[];
}

async function snapshot(): Promise<Snapshot> {
  const snap = await app.invoke<Snapshot | null>("session_snapshot", { serverId: await serverId() });
  if (!snap) throw new Error("no session snapshot");
  return snap;
}

/** The session of the person named `name`, as the current client sees it. */
export async function sessionOf(name: string): Promise<number> {
  let found: number | undefined;
  await browser.waitUntil(
    async () => {
      found = (await snapshot()).users.find((u) => u.name === name)?.session;
      return found !== undefined;
    },
    { timeout: 20_000, timeoutMsg: `${name} is not on the server` },
  );
  return found as number;
}

/** The current client's own session. */
export async function ownSession(): Promise<number> {
  return (await snapshot()).selfSession;
}

/** Waits until the own camera is in `state`; returns the view. */
export async function waitCameraState(state: CameraState, timeout = 20_000): Promise<CameraView> {
  const off: CameraView = { serverId: 0, state: "off", error: null, broadcast: null, viewers: [] };
  let view = off;
  await browser
    .waitUntil(
      async () => {
        view = (await cameraView()) ?? off;
        return view.state === state;
      },
      { timeout, interval: 100 },
    )
    .catch(() => {
      throw new Error(`the camera is ${JSON.stringify(view)}, want ${state}`);
    });
  return view;
}

/** The tile of `name` in the current client's room, or `undefined`. */
export async function tileOf(name: string): Promise<CameraTile | undefined> {
  const session = await sessionOf(name);
  return (await cameraRoom()).tiles.find((t) => t.sharer === session);
}

/** Waits until the tile of `name` is in `state` (default: playing). */
export async function waitTile(name: string, state: TileState = "playing", timeout = 30_000): Promise<CameraTile> {
  let tile: CameraTile | undefined;
  await browser
    .waitUntil(
      async () => {
        tile = await tileOf(name);
        return tile?.state === state;
      },
      { timeout, interval: 250 },
    )
    .catch(() => {
      throw new Error(`${name}'s tile is ${JSON.stringify(tile)}, want ${state}`);
    });
  return tile as CameraTile;
}

/** Waits until the current client has no tile for `name`. */
export async function waitNoTile(name: string, timeout = 15_000): Promise<void> {
  const session = await sessionOf(name).catch(() => undefined);
  await browser.waitUntil(
    async () => session === undefined || !(await cameraRoom()).tiles.some((t) => t.sharer === session),
    { timeout, interval: 250, timeoutMsg: `${name}'s tile is still there` },
  );
}

// ---------------------------------------------------------------- user steps

/** Opens Settings → Camera (the tab may carry a "New" pill after its name). */
export async function openCameraSettings(): Promise<void> {
  await openAllSettings();
  const tab = $("//button[@role='tab'][starts-with(normalize-space(.),'Camera')]");
  await tab.waitForClickable({ timeout: 10_000 });
  await tab.click();
  await $(
    "//h2[contains(@class,'settings-page__title')][normalize-space(.)='Camera']",
  ).waitForDisplayed({ timeout: 10_000 });
}

/** The switch labelled `label` on the open settings page. */
export function settingsSwitch(label: string) {
  return $(
    `//div[contains(concat(' ',normalize-space(@class),' '),' settings-switch ')][.//*[starts-with(normalize-space(.),"${label}")]]//input[@role='switch']`,
  );
}

/** Opens or closes the current client's camera room (joining the cameras needs it open). */
export async function setRoomOpen(open: boolean): Promise<void> {
  await app.invoke("camera_room_set_open", { serverId: await serverId(), open });
}

/** Opens the preview from the user panel's camera button. */
export async function openPreview(): Promise<void> {
  const button = $("button.camera-button[aria-label='Turn on camera']");
  await button.waitForClickable({ timeout: 20_000 });
  await button.click();
  await previewDialog().waitForDisplayed({ timeout: 10_000 });
}

/** The open camera preview dialog. */
export function previewDialog() {
  return $("dialog.cam-dialog");
}

/** Turns the camera on through the preview and waits until it is on air. */
export async function turnCameraOn(): Promise<CameraView> {
  await openPreview();
  const confirm = previewDialog().$(".app-dialog__actions .button--primary");
  await confirm.waitForEnabled({ timeout: 20_000 });
  await confirm.click();
  await waitForAbsent("dialog.cam-dialog", {
    timeout: 20_000, timeoutMsg: "the camera preview did not close",
  });
  return waitCameraState("onAir");
}

/** Turns the own camera off (the user-panel button) and waits until it is off. */
export async function turnCameraOff(): Promise<void> {
  await app.invoke("camera_turn_off", { serverId: await serverId() });
  await waitCameraState("off");
}

/** The whole visible text of the current window. */
export function pageText(): Promise<string> {
  return browser.execute(() => document.body.innerText);
}

/** Waits until the current window shows `text` somewhere. */
export async function waitText(text: string, timeout = 15_000): Promise<void> {
  await browser.waitUntil(async () => (await pageText()).includes(text), {
    timeout,
    timeoutMsg: `"${text}" never showed`,
  });
}

/**
 * Waits for the "Camera stopped: …" notice (section 5.6): an alert in the room bar or below the
 * camera mini bar, with a Dismiss button.
 */
export async function waitStoppedNotice(text: string, timeout = 15_000): Promise<void> {
  const notice = $(`//*[@role='alert'][contains(@class,'cam-stopped')][contains(., "${text}")]`);
  await notice.waitForDisplayed({ timeout, timeoutMsg: `the notice "${text}" never showed` });
  await notice.$(".//button[@aria-label='Dismiss']").waitForExist();
}

/** Changes settings of the current client through the settings command. */
export async function updateSettings(patch: Record<string, unknown>): Promise<void> {
  const settings = await app.invoke<Record<string, unknown>>("settings_get");
  await app.invoke("settings_update", { settings: { ...settings, ...patch } });
}

/** The log files of client `id`'s current profile, joined. */
export function clientLog(id: AppId): string {
  const dir = join(app.profile(id), "data", "app.mumcord.client", "logs");
  return readdirSync(dir)
    .map((f) => readFileSync(join(dir, f), "utf8"))
    .join("\n");
}

// ---------------------------------------------------------------- packet capture

export interface CapturedPacket {
  direction: "sent" | "received";
  kind: string;
  pluginData: boolean;
  sender: number | null;
  receivers: number[];
  decoded: string | null;
  rawHex: string;
}

/** Captures packets on the current client without opening the inspector window. */
export async function startCapture(): Promise<void> {
  await app.invoke("packet_capture_background", { enabled: true });
  await app.invoke("packet_capture_clear");
}

/** Everything captured so far on the current client. */
export async function captured(): Promise<CapturedPacket[]> {
  return (await app.invoke<{ packets: CapturedPacket[] }>("packet_capture_poll", { after: null }))
    .packets;
}

/** Captured `mumcord.camera` messages with their decoded fields. */
export async function cameraMessages(): Promise<
  { direction: string; sender: number | null; fields: Record<string, unknown> | null; raw: string }[]
> {
  return (await captured())
    .filter((p) => p.pluginData && p.kind.includes("mumcord.camera"))
    .map((p) => ({
      direction: p.direction,
      sender: p.sender,
      fields: p.decoded === null ? null : (JSON.parse(p.decoded) as Record<string, unknown>),
      raw: Buffer.from(p.rawHex.replace(/\s+/g, ""), "hex").toString("latin1"),
    }));
}

// ---------------------------------------------------------------- raw Mumble client

// Message types and field numbers of Mumble.proto that the raw client needs.
const MSG = { Version: 0, Authenticate: 2, Ping: 3, ServerSync: 5, UserState: 9, PluginData: 26 };

function varint(value: number | bigint): number[] {
  let v = BigInt(value);
  const out: number[] = [];
  do {
    let byte = Number(v & 0x7fn);
    v >>= 7n;
    if (v > 0n) byte |= 0x80;
    out.push(byte);
  } while (v > 0n);
  return out;
}

type Field = [number, number | bigint | boolean | string | Buffer];

function encode(fields: Field[]): Buffer {
  const out: number[] = [];
  for (const [no, value] of fields) {
    if (typeof value === "string" || Buffer.isBuffer(value)) {
      const bytes = typeof value === "string" ? Buffer.from(value, "utf8") : value;
      out.push(...varint((no << 3) | 2), ...varint(bytes.length), ...bytes);
    } else {
      out.push(...varint(no << 3), ...varint(typeof value === "boolean" ? Number(value) : value));
    }
  }
  return Buffer.from(out);
}

/**
 * The type field of a `mumcord.camera` document (`.t = "on";`, §6.5): Bovnar text, so a scripted
 * client reads it without a parser.
 */
function cameraType(document: string): string {
  return /\.t\s*=\s*"([a-z-]+)"/.exec(document)?.[1] ?? "";
}

/** The camera session id a `mumcord.camera` `on` document names (16 lowercase hex). */
export function cameraId(document: string): string {
  const id = /\.id\s*=\s*"([0-9a-f]{16})"/.exec(document)?.[1];
  if (id === undefined) throw new Error(`no camera id in '${document}'`);
  return id;
}

/** Varint and length-delimited fields of one protobuf message (enough for ServerSync/UserState). */
function decode(buf: Buffer): Map<number, (bigint | Buffer)[]> {
  const fields = new Map<number, (bigint | Buffer)[]>();
  let at = 0;
  const readVarint = (): bigint => {
    let result = 0n;
    let shift = 0n;
    for (;;) {
      const byte = buf[at++];
      result |= BigInt(byte & 0x7f) << shift;
      if ((byte & 0x80) === 0) return result;
      shift += 7n;
    }
  };
  while (at < buf.length) {
    const tag = Number(readVarint());
    const no = tag >> 3;
    let value: bigint | Buffer;
    switch (tag & 7) {
      case 0:
        value = readVarint();
        break;
      case 2: {
        const len = Number(readVarint());
        value = buf.subarray(at, at + len);
        at += len;
        break;
      }
      case 1:
        value = buf.subarray(at, at + 8);
        at += 8;
        break;
      case 5:
        value = buf.subarray(at, at + 4);
        at += 4;
        break;
      default:
        return fields;
    }
    fields.set(no, [...(fields.get(no) ?? []), value]);
  }
  return fields;
}

/**
 * A minimal Mumble client that sends hand-made plugin data (AC-8.26). The official clients and
 * mumbot only send well-formed documents, so the fuzzing needs its own sender.
 */
export class RawClient {
  private socket: TLSSocket | undefined;
  private pending = Buffer.alloc(0);
  session: number | undefined;
  readonly users = new Map<number, { name: string; channelId: number }>();
  /** Plugin data it received, oldest first: `dataId` and payload (a 0.8.x-style scripted viewer
   * reads the camera session id out of the `mumcord.camera` `on` document). */
  readonly received: { dataId: string; data: Buffer }[] = [];

  constructor(private readonly name: string) {}

  async connect(port: number): Promise<void> {
    const socket = tlsConnect({ host: "127.0.0.1", port, rejectUnauthorized: false });
    this.socket = socket;
    socket.on("data", (chunk: Buffer) => {
      this.receive(chunk);
    });
    await new Promise<void>((resolve, reject) => {
      socket.once("secureConnect", resolve);
      socket.once("error", reject);
    });
    // 1.5.0: version_v1 = 0x010500, version_v2 = 1 << 48 | 5 << 32.
    this.send(MSG.Version, encode([[1, 0x010500], [2, "fuzz"], [3, "Linux"], [5, (1n << 48n) | (5n << 32n)]]));
    this.send(MSG.Authenticate, encode([[1, this.name], [5, true]]));
    await browser.waitUntil(async () => Promise.resolve(this.session !== undefined), {
      timeout: 15_000,
      timeoutMsg: "the raw client never got its ServerSync",
    });
  }

  private receive(chunk: Buffer): void {
    this.pending = Buffer.concat([this.pending, chunk]);
    while (this.pending.length >= 6) {
      const type = this.pending.readUInt16BE(0);
      const len = this.pending.readUInt32BE(2);
      if (this.pending.length < 6 + len) return;
      const body = this.pending.subarray(6, 6 + len);
      this.pending = this.pending.subarray(6 + len);
      if (type === MSG.ServerSync) {
        const session = decode(body).get(1)?.[0];
        if (typeof session === "bigint") this.session = Number(session);
      } else if (type === MSG.UserState) {
        const f = decode(body);
        const session = Number(f.get(1)?.[0] ?? -1n);
        const prev = this.users.get(session);
        const name = f.get(3)?.[0];
        const channel = f.get(5)?.[0];
        this.users.set(session, {
          name: Buffer.isBuffer(name) ? name.toString("utf8") : (prev?.name ?? ""),
          channelId: typeof channel === "bigint" ? Number(channel) : (prev?.channelId ?? 0),
        });
      } else if (type === MSG.PluginData) {
        const f = decode(body);
        const data = f.get(3)?.[0];
        const dataId = f.get(4)?.[0];
        if (Buffer.isBuffer(data) && Buffer.isBuffer(dataId)) {
          this.received.push({ dataId: dataId.toString("utf8"), data });
        }
      }
    }
  }

  private send(type: number, body: Buffer): void {
    const head = Buffer.alloc(6);
    head.writeUInt16BE(type, 0);
    head.writeUInt32BE(body.length, 2);
    this.socket?.write(Buffer.concat([head, body]));
  }

  sessionOf(name: string): number {
    for (const [session, user] of this.users) if (user.name === name) return session;
    throw new Error(`the raw client does not see ${name}`);
  }

  /**
   * Waits for a `mumcord.camera` document of type `t` and returns it as text: Bovnar documents
   * are `.v = 1; .t = "on"; .id = "…";` (§6.5), so a scripted client can read them without a
   * parser.
   */
  async cameraDocument(t: string, timeout = 15_000): Promise<string> {
    let found = "";
    await browser.waitUntil(
      () => {
        const wanted = this.received.filter((p) => p.dataId === "mumcord.camera");
        const hit = wanted.find((p) => cameraType(p.data.toString("utf8")) === t);
        found = hit?.data.toString("utf8") ?? "";
        return Promise.resolve(hit !== undefined);
      },
      { timeout, timeoutMsg: `the raw client got no '${t}' camera document` },
    );
    return found;
  }

  /** Moves into the channel of `name`, so its camera messages count as same-channel. */
  async joinChannelOf(name: string): Promise<void> {
    const target = this.users.get(this.sessionOf(name));
    if (!target || this.session === undefined) throw new Error(`no channel for ${name}`);
    this.send(MSG.UserState, encode([[1, this.session], [5, target.channelId]]));
    await browser.waitUntil(
      async () => Promise.resolve(this.users.get(this.session ?? -1)?.channelId === target.channelId),
      { timeout: 10_000, timeoutMsg: "the raw client did not move" },
    );
  }

  /** Sends `data` as plugin data with `dataId` to `receiver`. */
  pluginData(receiver: number, dataId: string, data: Buffer): void {
    this.send(MSG.PluginData, encode([[1, this.session ?? 0], [2, receiver], [3, data], [4, dataId]]));
  }

  ping(): void {
    this.send(MSG.Ping, encode([[1, BigInt(Date.now())]]));
  }

  close(): void {
    this.socket?.destroy();
  }
}
