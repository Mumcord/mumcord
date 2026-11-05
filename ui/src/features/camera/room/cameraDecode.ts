/**
 * Which camera codecs the engine decodes for this window (FR-8.54): those the webview's WebCodecs
 * cannot, plus any codec WebCodecs failed on before its first picture. One list for every camera,
 * sent with `camera_set_decode_codecs` whenever it grows.
 */
import { getApi, type VideoCodec } from "../../../api";
import { VIDEO_CODECS, webCodecsCodecs } from "../../../api/screenFrames";

let inEngine: Set<VideoCodec> | null = null;
let started: Promise<void> | null = null;

function send(): void {
  if (inEngine === null) return;
  getApi()
    .cameraSetDecodeCodecs([...inEngine])
    .catch((error: unknown) => {
      console.warn("setting the camera decode codecs failed", error);
    });
}

/** Tells the engine once per window which codecs it decodes; later calls wait for that. */
export function ensureDecodeCodecs(): Promise<void> {
  started ??= webCodecsCodecs().then((inWebview) => {
    inEngine = new Set(VIDEO_CODECS.filter((codec) => !inWebview.includes(codec)));
    send();
  });
  return started;
}

/** WebCodecs failed on `codec` before showing a picture: the engine decodes it from now on. */
export function decodeInEngine(codec: VideoCodec): void {
  if (inEngine === null || inEngine.has(codec)) return;
  console.warn("WebCodecs cannot play this camera; the engine decodes it", codec);
  inEngine.add(codec);
  send();
}

/** Test helper. */
export function resetCameraDecode(): void {
  inEngine = null;
  started = null;
}
