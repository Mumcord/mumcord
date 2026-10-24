import { browser, expect } from "@wdio/globals";
import { app, axe } from "../../helpers/index.ts";

// Harness smoke test: the app starts in the container, renders its first screen (wizard on a fresh
// install) and an accessibility scan runs. Violations are only reported here; C7 owns the gate.
describe("smoke", () => {
  async function rendered(): Promise<void> {
    await expect(browser).toHaveTitle("Mumcord");
    const root = browser.$("#root");
    await browser.waitUntil(async () => (await root.getText()).trim().length > 0, {
      timeoutMsg: "app rendered nothing",
    });
  }

  it("renders the first screen", async () => {
    await rendered();
  });

  it("runs an axe scan", async () => {
    const violations = await axe.scan("smoke-first-screen");
    if (violations.length > 0) {
      console.warn(`axe: ${String(violations.length)} violation(s)\n${axe.summarize(violations)}`);
    }
  });

  it("restarts with the same profile", async () => {
    const before = app.profile();
    await app.restart();
    expect(app.profile()).toBe(before);
    await rendered();
  });
});
