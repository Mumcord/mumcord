/**
 * Focus mode (issue #163) hides the other columns but never the picture: the media views live in
 * the chat column, so the chat column itself stays and only the chat in it goes (`mediaOnly`).
 */
import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import "../i18n";
import { AppShell, type ShellSlots } from "./AppShell";

function slots(): ShellSlots {
  return {
    servers: <p>Servers</p>,
    channels: <p>Channels</p>,
    userPanel: <p>User panel</p>,
    chatStatus: null,
    chat: <section aria-label="Shared screen">Picture</section>,
    members: <p>Members</p>,
  };
}

/** Whether the element or one of its ancestors is hidden by the shell's stylesheet. */
function hiddenByCss(element: HTMLElement): boolean {
  for (let node: HTMLElement | null = element; node !== null; node = node.parentElement) {
    if (getComputedStyle(node).display === "none") return true;
  }
  return false;
}

describe("focus mode", () => {
  let style: HTMLStyleElement | null = null;
  afterEach(() => {
    style?.remove();
  });

  it("keeps the picture and hides the other columns", () => {
    style = document.createElement("style");
    style.textContent = readFileSync("ui/src/app/shell.css", "utf8");
    document.head.append(style);
    render(<AppShell slots={slots()} focus />);
    expect(hiddenByCss(screen.getByRole("region", { name: "Shared screen" }))).toBe(false);
    expect(hiddenByCss(screen.getByText("Channels"))).toBe(true);
    expect(hiddenByCss(screen.getByText("Members"))).toBe(true);
  });
});
