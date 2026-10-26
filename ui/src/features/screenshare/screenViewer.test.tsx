import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import type { ScreenFrame } from "../../api/screenFrames";
import { getServerState } from "../channels/serverStore";
import { connectedMock, resetAll } from "../channels/testUtils";
import { createFrameRenderer } from "./frameRenderer";
import { resetFloatingWidgets } from "../../components/floatingWidgetStore";
import { ScreenShareWidget } from "./ScreenShareViewer";
import { resetScreenShareStore } from "./sharesStore";
import { resetWatchingStore, startWatching } from "./watching";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: ArrayBuffer) => void = () => undefined;
  },
}));

const ANNA = 2;

class FakeDecoder {
  static instances: FakeDecoder[] = [];
  static isConfigSupported = vi.fn(() => Promise.resolve({ supported: true }));
  state: CodecState = "unconfigured";
  config: VideoDecoderConfig | null = null;
  chunks: { type: string; timestamp: number }[] = [];
  constructor(readonly init: VideoDecoderInit) {
    FakeDecoder.instances.push(this);
  }
  configure(config: VideoDecoderConfig) {
    this.config = config;
    this.state = "configured";
  }
  decode(chunk: { type: string; timestamp: number }) {
    this.chunks.push(chunk);
  }
  close() {
    this.state = "closed";
  }
  /** Like a real decoder: an error closes it, then reports. */
  fail() {
    this.state = "closed";
    this.init.error(new DOMException("bad frame"));
  }
  emit(width: number, height: number) {
    const picture = { displayWidth: width, displayHeight: height, close: vi.fn() };
    this.init.output(picture as unknown as VideoFrame);
    return picture;
  }
}

class FakeChunk {
  readonly type: string;
  readonly timestamp: number;
  constructor(init: { type: string; timestamp: number }) {
    this.type = init.type;
    this.timestamp = init.timestamp;
  }
}

class FakeImageData {
  constructor(
    readonly data: Uint8ClampedArray,
    readonly width: number,
    readonly height: number,
  ) {}
}

const context = { drawImage: vi.fn(), putImageData: vi.fn() };

function vp9(keyframe: boolean, width = 1280, height = 720): ScreenFrame {
  return {
    kind: "vp9",
    keyframe,
    temporalLayer: 0,
    timestampUs: 1000,
    width,
    height,
    data: new Uint8Array([1, 2, 3]),
  };
}

/** The 16-byte header of `src-tauri/src/screen_frames.rs`, then the payload. */
function message(kind: 1 | 2, keyframe: boolean, width: number, height: number, payload: number) {
  const buffer = new ArrayBuffer(16 + payload);
  const view = new DataView(buffer);
  view.setUint8(0, kind);
  view.setUint8(1, keyframe ? 1 : 0);
  view.setBigUint64(4, 5000n, true);
  view.setUint16(12, width, true);
  view.setUint16(14, height, true);
  return buffer;
}

beforeEach(() => {
  FakeDecoder.instances = [];
  context.drawImage.mockClear();
  context.putImageData.mockClear();
  vi.stubGlobal("VideoDecoder", FakeDecoder);
  vi.stubGlobal("EncodedVideoChunk", FakeChunk);
  vi.stubGlobal("ImageData", FakeImageData);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
    context as unknown as CanvasRenderingContext2D,
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("frame renderer", () => {
  const setup = () => {
    const canvas = document.createElement("canvas");
    const requestKeyframe = vi.fn();
    const onFirstPicture = vi.fn();
    const renderer = createFrameRenderer(canvas, { mode: "vp9", requestKeyframe, onFirstPicture });
    return { canvas, renderer, requestKeyframe, onFirstPicture };
  };

  it("starts decoding on a keyframe and asks for one once while waiting", () => {
    const { renderer, requestKeyframe } = setup();
    renderer.push(vp9(false));
    renderer.push(vp9(false));
    expect(FakeDecoder.instances).toHaveLength(0);
    expect(requestKeyframe).toHaveBeenCalledTimes(1);

    renderer.push(vp9(true));
    renderer.push(vp9(false));
    const [decoder] = FakeDecoder.instances;
    expect(decoder?.config).toMatchObject({ codec: "vp09.00.10.08", codedWidth: 1280 });
    expect(decoder?.chunks.map((chunk) => chunk.type)).toEqual(["key", "delta"]);
  });

  it("draws decoded pictures and frees them", () => {
    const { canvas, renderer, onFirstPicture } = setup();
    renderer.push(vp9(true));
    const picture = FakeDecoder.instances[0]?.emit(1280, 720);
    expect(context.drawImage).toHaveBeenCalledWith(picture, 0, 0, 1280, 720);
    expect(picture?.close).toHaveBeenCalled();
    expect([canvas.width, canvas.height]).toEqual([1280, 720]);
    expect(onFirstPicture).toHaveBeenCalledTimes(1);
  });

  it("asks for a keyframe after a decoder error and restarts on it", () => {
    const { renderer, requestKeyframe } = setup();
    renderer.push(vp9(true));
    FakeDecoder.instances[0]?.fail();
    expect(requestKeyframe).toHaveBeenCalledTimes(1);
    renderer.push(vp9(false));
    expect(FakeDecoder.instances[0]?.chunks).toHaveLength(1);

    renderer.push(vp9(true));
    expect(FakeDecoder.instances).toHaveLength(2);
    expect(FakeDecoder.instances[1]?.chunks.map((chunk) => chunk.type)).toEqual(["key"]);
  });

  it("starts a new decoder when a keyframe changes the size", () => {
    const { renderer } = setup();
    renderer.push(vp9(true));
    renderer.push(vp9(true));
    expect(FakeDecoder.instances).toHaveLength(1);
    renderer.push(vp9(true, 854, 480));
    expect(FakeDecoder.instances[0]?.state).toBe("closed");
    expect(FakeDecoder.instances[1]?.config).toMatchObject({ codedWidth: 854, codedHeight: 480 });
  });

  it("ignores frames after close", () => {
    const { renderer } = setup();
    renderer.push(vp9(true));
    renderer.close();
    renderer.push(vp9(true));
    expect(FakeDecoder.instances).toHaveLength(1);
    expect(FakeDecoder.instances[0]?.state).toBe("closed");
  });
});

describe("screen share viewer", () => {
  let api: MockApi;
  let channel: { onmessage: (message: ArrayBuffer) => void } | null;
  let name: string;

  beforeEach(async () => {
    api = await connectedMock({ bindStore: true });
    name = getServerState().users.get(ANNA)?.name ?? "";
    channel = null;
    vi.spyOn(api, "screenShareWatch").mockImplementation((_server, _session, frames) => {
      channel = frames;
      return Promise.resolve(null);
    });
    vi.spyOn(api, "screenShareUnwatch").mockResolvedValue(null);
    vi.spyOn(api, "screenShareSetDecodeFallback").mockResolvedValue(null);
    vi.spyOn(api, "screenShareRequestKeyframe").mockResolvedValue(null);
  });

  afterEach(() => {
    resetWatchingStore();
    resetScreenShareStore();
    resetFloatingWidgets();
    resetAll();
  });

  function Main() {
    return (
      <>
        <div className="chat">
          <p>chat</p>
          <textarea aria-label="Message" />
        </div>
        <ScreenShareWidget />
      </>
    );
  }

  const open = async () => {
    const view = render(<Main />);
    act(() => {
      startWatching(1, ANNA);
    });
    await waitFor(() => {
      expect(channel).not.toBeNull();
    });
    return view;
  };

  const send = (buffer: ArrayBuffer) => {
    act(() => {
      channel?.onmessage(buffer);
    });
  };

  it("opens in a floating widget, keeps the chat visible and decodes from the first keyframe", async () => {
    await open();
    expect(screen.getByText("chat")).toBeVisible();
    const viewer = screen.getByRole("region", { name: `Screen share of ${name}` });
    expect(viewer).toHaveClass("floating-widget");
    expect(api.screenShareWatch).toHaveBeenCalledWith(1, ANNA, expect.anything());
    expect(screen.getByRole("status")).toHaveTextContent(`Connecting to ${name}'s screen`);

    send(message(1, false, 1280, 720, 4));
    expect(api.screenShareRequestKeyframe).toHaveBeenCalledWith(1, ANNA);
    send(message(1, true, 1280, 720, 4));
    act(() => {
      FakeDecoder.instances[0]?.emit(1280, 720);
    });
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(screen.getByRole("img", { name: `Shared screen of ${name}` })).toBeInTheDocument();
    expect(api.screenShareSetDecodeFallback).not.toHaveBeenCalled();
  });

  it("lets the engine decode when the webview has no VP9", async () => {
    vi.stubGlobal("VideoDecoder", undefined);
    await open();
    await waitFor(() => {
      expect(api.screenShareSetDecodeFallback).toHaveBeenCalledWith(1, ANNA, true);
    });
    send(message(1, true, 2, 2, 4));
    expect(context.putImageData).not.toHaveBeenCalled();
    send(message(2, false, 2, 2, 16));
    expect(context.putImageData).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
  });

  it("stops watching and frees the decoder on Stop watching", async () => {
    await open();
    send(message(1, true, 1280, 720, 4));
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(screen.getByText("chat")).toBeInTheDocument();
    expect(api.screenShareUnwatch).toHaveBeenCalledWith(1, ANNA);
    expect(FakeDecoder.instances[0]?.state).toBe("closed");
  });

  it("cleans up on unmount", async () => {
    const view = await open();
    send(message(1, true, 1280, 720, 4));
    view.unmount();
    expect(api.screenShareUnwatch).toHaveBeenCalledWith(1, ANNA);
    expect(FakeDecoder.instances[0]?.state).toBe("closed");
  });

  it("stops watching when unmounted before the stream started", async () => {
    let resolveWatch: (value: null) => void = () => undefined;
    vi.mocked(api.screenShareWatch).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveWatch = resolve;
        }),
    );
    const view = render(<Main />);
    act(() => {
      startWatching(1, ANNA);
    });
    await waitFor(() => {
      expect(api.screenShareWatch).toHaveBeenCalled();
    });
    // Right away, so the engine gives up its attempt instead of opening a stream nobody watches.
    view.unmount();
    expect(api.screenShareUnwatch).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveWatch(null);
      await Promise.resolve();
    });
    expect(api.screenShareUnwatch).toHaveBeenCalledTimes(1);
  });

  it("says when the share is full", async () => {
    api.mock.failNext("screenShareWatch", "shareFull");
    vi.mocked(api.screenShareWatch).mockRestore();
    render(<Main />);
    act(() => {
      startWatching(1, ANNA);
    });
    expect(await screen.findByText("This share is full.")).toBeInTheDocument();
    expect(api.screenShareUnwatch).not.toHaveBeenCalled();
  });

  it("explains a failed direct connection and offers Try again and Stop watching", async () => {
    api.mock.failNext("screenShareWatch", "directUnavailable", "noDirectPath");
    vi.mocked(api.screenShareWatch).mockRestore();
    render(<Main />);
    act(() => {
      startWatching(1, ANNA);
    });
    expect(
      await screen.findByText(
        `Couldn't reach ${name} directly. Their network may block direct connections.`,
      ),
    ).toBeInTheDocument();
    // A new attempt; the mock engine has no screen sharing, so it fails differently.
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(`${name}'s screen could not be opened.`)).toBeInTheDocument();
    const stop = screen.getAllByRole("button", { name: "Stop watching" });
    fireEvent.click(stop[stop.length - 1] as HTMLElement);
    expect(screen.queryByRole("region", { name: `Screen share of ${name}` })).toBeNull();
  });

  it("says when the sharer's Mumcord is too old", async () => {
    api.mock.failNext("screenShareWatch", "directUnavailable", "peerUnsupported");
    vi.mocked(api.screenShareWatch).mockRestore();
    render(<Main />);
    act(() => {
      startWatching(1, ANNA);
    });
    expect(
      await screen.findByText(`${name}'s Mumcord is too old to share their screen with you.`),
    ).toBeInTheDocument();
  });

  describe("without a picture", () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    const unreachable = () =>
      screen.queryByText(
        `Couldn't reach ${name} directly. Their network may block direct connections.`,
      );

    it("gives up after 15 s while the engine still connects, then can try again", async () => {
      vi.mocked(api.screenShareWatch).mockImplementation(() => new Promise(() => undefined));
      render(<Main />);
      act(() => {
        startWatching(1, ANNA);
      });
      await waitFor(() => {
        expect(api.screenShareWatch).toHaveBeenCalledTimes(1);
      });
      act(() => {
        vi.advanceTimersByTime(14_000);
      });
      expect(unreachable()).toBeNull();
      act(() => {
        vi.advanceTimersByTime(1_000);
      });
      expect(unreachable()).toBeInTheDocument();
      // The engine's attempt ends too.
      expect(api.screenShareUnwatch).toHaveBeenCalledTimes(1);
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      await waitFor(() => {
        expect(api.screenShareWatch).toHaveBeenCalledTimes(2);
      });
      expect(unreachable()).toBeNull();
    });

    it("gives up when the stream opened but no picture came", async () => {
      await open();
      act(() => {
        vi.advanceTimersByTime(15_000);
      });
      expect(unreachable()).toBeInTheDocument();
      expect(api.screenShareUnwatch).toHaveBeenCalledTimes(1);
    });

    it("keeps waiting while the sharer paused", async () => {
      await open();
      act(() => {
        api.mock.emit("screenSharesChanged", {
          serverId: 1,
          shares: [
            {
              session: ANNA,
              paused: true,
              sound: false,
              soundAvailable: false,
              viewers: [],
              blocked: null,
            },
          ],
        });
      });
      act(() => {
        vi.advanceTimersByTime(30_000);
      });
      expect(unreachable()).toBeNull();
      expect(screen.getByText(`${name} paused the share.`)).toBeInTheDocument();
    });

    it("does not time out once pictures arrive", async () => {
      await open();
      send(message(1, true, 1280, 720, 3));
      act(() => {
        FakeDecoder.instances[0]?.emit(1280, 720);
      });
      act(() => {
        vi.advanceTimersByTime(30_000);
      });
      expect(unreachable()).toBeNull();
      expect(api.screenShareUnwatch).not.toHaveBeenCalled();
    });
  });

  it("closes when the share ends", async () => {
    await open();
    act(() => {
      api.mock.emit("screenSharesChanged", { serverId: 1, shares: [] });
    });
    expect(screen.getByText("chat")).toBeInTheDocument();
    expect(api.screenShareUnwatch).toHaveBeenCalledWith(1, ANNA);
  });

  it("shows when the sharer paused", async () => {
    await open();
    act(() => {
      api.mock.emit("screenSharesChanged", {
        serverId: 1,
        shares: [
          {
            session: ANNA,
            paused: true,
            sound: false,
            soundAvailable: false,
            viewers: [1],
            blocked: null,
          },
        ],
      });
    });
    expect(screen.getByRole("status")).toHaveTextContent(`${name} paused the share.`);
  });

  it("toggles full screen with F and the button", async () => {
    const requestFullscreen = vi.fn(() => Promise.resolve());
    HTMLElement.prototype.requestFullscreen = requestFullscreen;
    await open();
    const viewer = screen.getByRole("region", { name: `Screen share of ${name}` });
    fireEvent.keyDown(viewer, { key: "f" });
    expect(requestFullscreen).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Full screen" }));
    expect(requestFullscreen).toHaveBeenCalledTimes(2);
  });

  it("collapses to a slim bar without stopping the stream", async () => {
    await open();
    const picture = screen.getByRole("img", { name: `Shared screen of ${name}` });
    fireEvent.click(screen.getByRole("button", { name: "Collapse" }));
    expect(picture).not.toBeVisible();
    expect(screen.getByText("Live")).toBeInTheDocument();
    expect(api.screenShareUnwatch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Expand" }));
    expect(picture).toBeVisible();
  });

  it("shows icon-only title-bar buttons with themed tooltips", async () => {
    await open();
    for (const name of ["Full screen", "Collapse", "Stop watching"]) {
      const button = screen.getByRole("button", { name });
      expect(button.textContent).not.toContain(name);
      expect(button.querySelector("[aria-hidden='true']")).not.toBeNull();
      expect(button).toHaveAttribute("data-tooltip", name);
    }
  });
});
