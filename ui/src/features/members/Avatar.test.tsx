import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { Avatar } from "./Avatar";

describe("Avatar", () => {
  it("shows the initial without an image", () => {
    const { container } = render(<Avatar name="anna" />);
    expect(container.querySelector(".avatar")?.getAttribute("data-initial")).toBe("A");
    // No DOM text: rows labelled by the name alone must not show extra text (WCAG 2.5.3).
    expect(container.textContent).toBe("");
    expect(container.querySelector("img")).toBeNull();
  });

  it("falls back to the initial when the image fails to load", () => {
    const { container } = render(<Avatar name="Anna" src="data:image/png;base64,broken" />);
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    if (img) fireEvent.error(img);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector(".avatar")?.getAttribute("data-initial")).toBe("A");
  });

  it("picks a stable palette color per name, case-insensitively", () => {
    const colorOf = (name: string): string =>
      render(<Avatar name={name} />)
        .container.querySelector(".avatar")
        ?.getAttribute("style") ?? "";
    expect(colorOf("Anna")).toBe(colorOf("anna "));
    const used = new Set(["a", "bob", "Carol", "dave", "Eve", "frank", "Grace"].map(colorOf));
    expect(used.size).toBeGreaterThan(1);
    for (const style of used) expect(style).toMatch(/--color-avatar-[1-8]\)/);
  });

  it("uses no border and a non-talking palette", () => {
    const css = readFileSync("ui/src/features/members/members.css", "utf8");
    const tokens = readFileSync("ui/src/theme/tokens.css", "utf8");
    const avatarRule = /\.avatar \{[^}]*\}/.exec(css)?.[0] ?? "";
    expect(avatarRule).not.toContain("border:");
    const talking = [...tokens.matchAll(/--color-talking: (#\w+)/g)].map((m) => m[1]);
    const palette = [...tokens.matchAll(/--color-avatar-\d: (#\w+)/g)].map((m) => m[1]);
    expect(palette).toHaveLength(16);
    for (const c of palette) expect(talking).not.toContain(c);
  });
});
