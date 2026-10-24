import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Avatar } from "./Avatar";

describe("Avatar", () => {
  it("shows the initial without an image", () => {
    const { container } = render(<Avatar name="anna" />);
    expect(container.textContent).toBe("A");
    expect(container.querySelector("img")).toBeNull();
  });

  it("falls back to the initial when the image fails to load", () => {
    const { container } = render(<Avatar name="Anna" src="data:image/png;base64,broken" />);
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    if (img) fireEvent.error(img);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toBe("A");
  });
});
