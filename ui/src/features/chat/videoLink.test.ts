import { describe, expect, it } from "vitest";
import { canonicalVideoUrl } from "./videoLink";

describe("canonicalVideoUrl", () => {
  it("canonicalises the player addresses of the card services", () => {
    expect(canonicalVideoUrl("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?autoplay=1")).toBe(
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    expect(canonicalVideoUrl("https://youtu.be/dQw4w9WgXcQ?si=abc")).toBe(
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    expect(canonicalVideoUrl("https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=42")).toBe(
      "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    );
    expect(canonicalVideoUrl("https://www.youtube.com/shorts/dQw4w9WgXcQ")).toBe(
      "https://www.youtube.com/shorts/dQw4w9WgXcQ",
    );
    expect(canonicalVideoUrl("https://player.twitch.tv/?channel=someone&parent=localhost")).toBe(
      "https://www.twitch.tv/someone",
    );
    expect(canonicalVideoUrl("https://clips.twitch.tv/embed?clip=SomeClipId1234567890")).toBe(
      "https://clips.twitch.tv/SomeClipId1234567890",
    );
  });

  it("leaves anything that is not a video of the cards as the message wrote it", () => {
    expect(canonicalVideoUrl("https://example.test/page?x=1")).toBe(
      "https://example.test/page?x=1",
    );
    expect(canonicalVideoUrl("https://www.youtube.com/@someone")).toBe(
      "https://www.youtube.com/@someone",
    );
    expect(canonicalVideoUrl("https://www.twitch.tv/someone/about")).toBe(
      "https://www.twitch.tv/someone/about",
    );
    expect(
      canonicalVideoUrl("https://music.youtube.com/embed/dQw4w9WgXcQ?list=RDdQw4w9WgXcQ"),
    ).toBe("https://music.youtube.com/embed/dQw4w9WgXcQ?list=RDdQw4w9WgXcQ");
    expect(canonicalVideoUrl("not a url")).toBe("not a url");
  });
});
