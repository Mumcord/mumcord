import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { AudioCard } from "./AudioCard";
import { dataUrlToBlob, formatSize, formatTime, peaksOf } from "./audioPlayer";
import { clearPreviews } from "./links";

const URL_A = "https://files.example.org/demos/night-drive.mp3";
const URL_B = "https://files.example.org/demos/day-ride.ogg";
/** "ID3" and a few zero bytes: enough for the player, which never decodes it in jsdom. */
const FILE = "data:audio/mpeg;base64,SUQzBAAAAAAA";

let api: MockApi;
let play: MockInstance<HTMLMediaElement["play"]>;
let pause: MockInstance<HTMLMediaElement["pause"]>;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  clearPreviews();
  api = createMockApi();
  setApi(api);
  vi.spyOn(api, "audioInfo").mockImplementation((url) =>
    Promise.resolve({ url, format: "mp3", size: 4_200_000 }),
  );
  vi.spyOn(api, "audioFile").mockResolvedValue(FILE);
  // jsdom has no media playback: play and pause fire the events a browser would, like a
  // browser in a later task (firing them during the call would nest act() inside an effect).
  play = vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(function (
    this: HTMLMediaElement,
  ) {
    setTimeout(() => this.dispatchEvent(new Event("play")), 0);
    return Promise.resolve();
  });
  pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(function (
    this: HTMLMediaElement,
  ) {
    setTimeout(() => this.dispatchEvent(new Event("pause")), 0);
  });
  URL.createObjectURL = vi.fn(() => "blob:mumcord/1");
  URL.revokeObjectURL = vi.fn();
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

/** Clicks Play once the card knows the file (the button is disabled until then). */
async function clickPlay(name: string) {
  const button = await screen.findByRole("button", { name: `Play ${name}` });
  await waitFor(() => {
    expect(button).toBeEnabled();
  });
  fireEvent.click(button);
}

function renderCard(url = URL_A, onDismiss = vi.fn()) {
  return render(
    <main>
      <AudioCard url={url} tabIndex={0} onDismiss={onDismiss} />
    </main>,
  );
}

describe("AudioCard", () => {
  it("shows name, host and size, and fetches the file only on Play", async () => {
    const { container } = renderCard();
    const card = screen.getByRole("article", { name: "Audio file: night-drive.mp3" });
    await waitFor(() => {
      expect(card).toHaveTextContent("Audio file · files.example.org · 4.2 MB");
    });
    expect(api.audioFile).not.toHaveBeenCalled();
    expect(container.querySelector("audio")).toBeNull();
    expect(await axeViolations(container)).toEqual([]);

    await clickPlay("night-drive.mp3");
    await screen.findByRole("button", { name: "Pause night-drive.mp3" });
    expect(api.audioFile).toHaveBeenCalledWith(URL_A);
    expect(container.querySelector("audio")?.getAttribute("src")).toBe("blob:mumcord/1");
    expect(card).toHaveTextContent("Playing · files.example.org");
    expect(play).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Pause night-drive.mp3" }));
    expect(pause).toHaveBeenCalled();
    await screen.findByRole("button", { name: "Play night-drive.mp3" });
  });

  it("seeks and sets the volume on the audio element", async () => {
    const { container } = renderCard();
    await clickPlay("night-drive.mp3");
    await screen.findByRole("button", { name: "Pause night-drive.mp3" });
    const audio = container.querySelector("audio");
    if (audio === null) throw new Error("no audio element");
    Object.defineProperty(audio, "duration", { value: 222 });
    fireEvent.loadedMetadata(audio);
    const seek = screen.getByRole("slider", { name: "Position" });
    expect(seek).toHaveAttribute("aria-valuetext", "0:00 of 3:42");
    fireEvent.change(seek, { target: { value: "84" } });
    expect(audio.currentTime).toBe(84);
    fireEvent.change(screen.getByRole("slider", { name: "Volume" }), { target: { value: "0.4" } });
    expect(audio.volume).toBeCloseTo(0.4);
    fireEvent.click(screen.getByRole("button", { name: "Mute" }));
    expect(audio.muted).toBe(true);
  });

  it("plays only one file at a time", async () => {
    render(
      <main>
        <AudioCard url={URL_A} tabIndex={0} onDismiss={vi.fn()} />
        <AudioCard url={URL_B} tabIndex={0} onDismiss={vi.fn()} />
      </main>,
    );
    await clickPlay("night-drive.mp3");
    await screen.findByRole("button", { name: "Pause night-drive.mp3" });
    await clickPlay("day-ride.ogg");
    await screen.findByRole("button", { name: "Pause day-ride.ogg" });
    await screen.findByRole("button", { name: "Play night-drive.mp3" });
  });

  it("shows the error state when the link is not a playable file and retries", async () => {
    vi.mocked(api.audioInfo).mockResolvedValueOnce(null);
    const opened = vi.spyOn(api, "openLink").mockResolvedValue(null);
    const { container } = renderCard();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Can't play this file");
    expect(alert).toHaveTextContent("too large or in a format Mumcord can't play");
    expect(await axeViolations(container)).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "Open in browser" }));
    expect(opened).toHaveBeenCalledWith(URL_A);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    const again = await screen.findByRole("button", { name: "Play night-drive.mp3" });
    await waitFor(() => {
      expect(again).toBeEnabled();
    });
    expect(api.audioInfo).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows the error state when the file cannot be fetched or played", async () => {
    vi.mocked(api.audioFile).mockResolvedValueOnce(null);
    renderCard();
    await clickPlay("night-drive.mp3");
    expect(await screen.findByRole("alert")).toHaveTextContent("Can't play this file");
  });

  it("saves the file where the user chooses", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("/home/me/night-drive.mp3");
    const save = vi.spyOn(api, "audioSave").mockResolvedValue(null);
    renderCard();
    fireEvent.click(await screen.findByRole("button", { name: "Save night-drive.mp3" }));
    await waitFor(() => {
      expect(save).toHaveBeenCalledWith(URL_A, "/home/me/night-drive.mp3");
    });
  });
});

describe("audio player helpers", () => {
  it("formats times and sizes", () => {
    expect(formatTime(0)).toBe("0:00");
    expect(formatTime(222.7)).toBe("3:42");
    expect(formatTime(3725)).toBe("1:02:05");
    expect(formatTime(null)).toBe("–:––");
    expect(formatTime(Number.NaN)).toBe("–:––");
    expect(formatSize(4_200_000, "en")).toBe("4.2 MB");
    expect(formatSize(420_000, "en")).toBe("420 kB");
    expect(formatSize(4_200_000, "de")).toBe("4,2 MB");
  });

  it("turns only base64 data URLs into blobs of their type", () => {
    const blob = dataUrlToBlob(FILE);
    expect(blob?.type).toBe("audio/mpeg");
    expect(blob?.size).toBe(9);
    expect(dataUrlToBlob("https://files.example.org/a.mp3")).toBeNull();
    expect(dataUrlToBlob("data:audio/mpeg;base64,%%%")).toBeNull();
  });

  it("scales the waveform to the loudest bar", () => {
    const samples = new Float32Array([0, 0.5, -0.25, 0.1]);
    expect(peaksOf(samples, 2)).toEqual([1, 0.5]);
    expect(peaksOf(new Float32Array(4), 2)).toEqual([0, 0]);
  });
});
