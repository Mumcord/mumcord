import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type LinkPreview, type MockApi, type MusicEmbed } from "../../api";
import { applyLanguage } from "../../i18n";
import { axeViolations } from "../../test/axe";
import { musicPlayer } from "./links";
import { MusicCard } from "./MusicCard";

const COVER = "data:image/jpeg;base64,/9j/4AAQ";

function preview(url: string, music: MusicEmbed, title: string | null = "Midnight Signals") {
  const card: LinkPreview = {
    url,
    siteName: null,
    title,
    description: "Lumen Park · Single · 2025",
    image: COVER,
    video: null,
    music,
    isImage: false,
    isAudio: false,
  };
  return card;
}

const SPOTIFY: MusicEmbed = {
  service: "spotify",
  kind: "track",
  playerUrl: "https://open.spotify.com/embed/track/4uLU6hMCjMI75M1A2tKUQC",
  artist: null,
};

const SOUNDCLOUD: MusicEmbed = {
  service: "soundcloud",
  kind: "playlist",
  playerUrl:
    "https://w.soundcloud.com/player/?url=https%3A%2F%2Fsoundcloud.com%2Flumen-park%2Fsets%2Fnight&auto_play=true&visual=false",
  artist: "Lumen Park",
};

const YOUTUBE_MUSIC: MusicEmbed = {
  service: "youtubeMusic",
  kind: "track",
  playerUrl: "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1",
  artist: "Lumen Park - Topic",
};

const APPLE: MusicEmbed = { service: "appleMusic", kind: "album", playerUrl: null, artist: null };

function renderCard(card: LinkPreview, music: MusicEmbed, onDismiss = vi.fn()) {
  return render(
    <main>
      <MusicCard preview={card} music={music} tabIndex={0} onDismiss={onDismiss} />
    </main>,
  );
}

let api: MockApi;

beforeAll(async () => {
  await applyLanguage("en");
});

beforeEach(() => {
  api = createMockApi();
  setApi(api);
});

afterEach(() => {
  vi.restoreAllMocks();
  setApi(undefined);
});

describe("musicPlayer", () => {
  it("runs Spotify and SoundCloud in the app window and YouTube Music in the player page", () => {
    expect(musicPlayer(SPOTIFY)).toEqual({ via: "direct", url: SPOTIFY.playerUrl });
    expect(musicPlayer(SOUNDCLOUD)).toEqual({ via: "direct", url: SOUNDCLOUD.playerUrl });
    expect(musicPlayer(YOUTUBE_MUSIC)).toEqual({ via: "page", url: YOUTUBE_MUSIC.playerUrl });
    expect(musicPlayer(APPLE)).toBeNull();
  });

  it("refuses players outside the known embed origins and paths", () => {
    const wrong = (playerUrl: string, music = SPOTIFY) => musicPlayer({ ...music, playerUrl });
    expect(wrong("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC")).toBeNull();
    expect(wrong("https://soundcloud.com/lumen-park/x", SOUNDCLOUD)).toBeNull();
    expect(wrong("https://evil.example/embed/track/x")).toBeNull();
    expect(wrong("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ")).toBeNull();
    expect(wrong("not a url")).toBeNull();
  });
});

describe("MusicCard", () => {
  it("shows badge, title, details and the third-party host, and loads nothing before Play", async () => {
    const { container } = renderCard(
      preview("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC", SPOTIFY),
      SPOTIFY,
    );
    const card = screen.getByRole("article", { name: "Spotify: Midnight Signals" });
    expect(card).toHaveTextContent("Lumen Park · Single · 2025");
    expect(card).toHaveTextContent("Third-party content from open.spotify.com");
    expect(card).toHaveTextContent("Playback limits are set by the service");
    expect(card.querySelector("img")?.getAttribute("src")).toBe(COVER);
    expect(container.querySelector("iframe")).toBeNull();
    expect(await axeViolations(container)).toEqual([]);
  });

  it("loads the official embed after Play and removes it on Stop", () => {
    const { container } = renderCard(
      preview("https://soundcloud.com/lumen-park/sets/night", SOUNDCLOUD, "Night"),
      SOUNDCLOUD,
    );
    expect(screen.getByRole("article")).toHaveTextContent("Lumen Park · Playlist");
    fireEvent.click(screen.getByRole("button", { name: "Play Night on SoundCloud" }));
    const frame = screen.getByTitle("SoundCloud player: Night");
    expect(frame.getAttribute("src")).toBe(SOUNDCLOUD.playerUrl);
    expect(frame.getAttribute("sandbox")).not.toContain("allow-popups");
    expect(frame.style.height).toBe("450px");
    expect(screen.getByRole("article")).toHaveTextContent(
      "Playing on your output device, not into your microphone",
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop Night" }));
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("stops one card when another one starts", () => {
    render(
      <main>
        <MusicCard
          preview={preview("https://open.spotify.com/track/a", SPOTIFY, "One")}
          music={SPOTIFY}
          tabIndex={0}
          onDismiss={vi.fn()}
        />
        <MusicCard
          preview={preview("https://soundcloud.com/a/b", SOUNDCLOUD, "Two")}
          music={SOUNDCLOUD}
          tabIndex={0}
          onDismiss={vi.fn()}
        />
      </main>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Play One on Spotify" }));
    expect(screen.getByTitle("Spotify player: One")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Play Two on SoundCloud" }));
    expect(screen.queryByTitle("Spotify player: One")).toBeNull();
    expect(screen.getByTitle("SoundCloud player: Two")).toBeInTheDocument();
  });

  it("gives services without an embed a card with Open in and no player", async () => {
    const url = "https://music.apple.com/us/album/glass-harbour/1700000000";
    const opened = vi.spyOn(api, "openLink").mockResolvedValue(null);
    const { container } = renderCard(preview(url, APPLE, "Glass Harbour"), APPLE);
    const card = screen.getByRole("article", { name: "Apple Music: Glass Harbour" });
    expect(card).toHaveTextContent("No player available for this service");
    expect(card).toHaveTextContent("Third-party content from music.apple.com");
    expect(screen.queryByRole("button", { name: /^Play/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open in Apple Music: Glass Harbour" }));
    expect(opened).toHaveBeenCalledWith(url);
    expect(await axeViolations(container)).toEqual([]);
  });

  it("names a card without metadata after its service and labels other music pages", () => {
    const other: MusicEmbed = { service: "other", kind: "track", playerUrl: null, artist: null };
    renderCard(preview("https://tunes.example/x", other, null), other);
    const card = screen.getByRole("article", { name: "Music link: Music link" });
    expect(card).toHaveTextContent("Open in browser");
  });

  it("calls onDismiss from the x button", () => {
    const onDismiss = vi.fn();
    renderCard(preview("https://open.spotify.com/track/a", SPOTIFY), SPOTIFY, onDismiss);
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
