import { describe, expect, it } from "vitest";
import { gifSource } from "./gifSource";

describe("gifSource", () => {
  it("recognises Giphy pages and media and links to the page", () => {
    const page = { brand: "giphy", href: "https://giphy.com/gifs/1ojn1S7BTXUry8elNi" };
    expect(gifSource("https://giphy.com/gifs/meme-kid-1ojn1S7BTXUry8elNi")).toEqual(page);
    expect(gifSource("https://media2.giphy.com/media/1ojn1S7BTXUry8elNi/giphy.gif")).toEqual(page);
    expect(gifSource("https://media.giphy.com/media/v1.Y2lk/1ojn1S7BTXUry8elNi/giphy.gif")).toEqual(
      page,
    );
    expect(gifSource("https://i.giphy.com/1ojn1S7BTXUry8elNi.gif")).toEqual(page);
  });

  it("recognises Tenor pages and media", () => {
    const view = "https://tenor.com/view/cat-dance-12345678";
    expect(gifSource(view)).toEqual({ brand: "tenor", href: view });
    expect(gifSource("https://media.tenor.com/abc/cat.gif")?.brand).toBe("tenor");
    expect(gifSource("https://c.tenor.com/abc/cat.gif")?.brand).toBe("tenor");
  });

  it("ignores other hosts, look-alikes and bad input", () => {
    expect(gifSource("https://pics.example/cat.gif")).toBeNull();
    expect(gifSource("https://evilgiphy.com/gifs/1ojn1S7BTXUry8elNi")).toBeNull();
    expect(gifSource("https://giphy.com/search/cats")).toBeNull();
    expect(gifSource("https://tenor.com/search/cats")).toBeNull();
    expect(gifSource("javascript:alert(1)")).toBeNull();
    expect(gifSource("nope")).toBeNull();
  });
});
