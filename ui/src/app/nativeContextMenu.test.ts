import { describe, expect, it } from "vitest";
import { shouldSuppressNativeMenu } from "./nativeContextMenu";

function el(html: string): Element {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host.firstElementChild?.querySelector("span") ?? host;
}

describe("shouldSuppressNativeMenu", () => {
  it("suppresses on a plain element", () => {
    expect(shouldSuppressNativeMenu(el("<p><span>x</span></p>"), "")).toBe(true);
  });
  it("suppresses when there is no element target", () => {
    expect(shouldSuppressNativeMenu(null, "")).toBe(true);
  });
  it("keeps the menu in input, textarea and contenteditable", () => {
    expect(shouldSuppressNativeMenu(document.createElement("input"), "")).toBe(false);
    expect(shouldSuppressNativeMenu(document.createElement("textarea"), "")).toBe(false);
    expect(
      shouldSuppressNativeMenu(el("<div contenteditable='true'><span>x</span></div>"), ""),
    ).toBe(false);
  });
  it("keeps the menu when text is selected", () => {
    expect(shouldSuppressNativeMenu(el("<p><span>x</span></p>"), "hello")).toBe(false);
  });
});
