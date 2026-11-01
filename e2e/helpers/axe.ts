import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";
import { browser } from "@wdio/globals";
import type { AxeResults, ElementContext, Result, RunOptions } from "axe-core";
import { artifactsDir } from "./paths.ts";

const axeSource = readFileSync(
  createRequire(import.meta.url).resolve("axe-core/axe.min.js"),
  "utf8",
);

/** WCAG 2.2 AA (and below) rule tags, the bar set by the spec (Q4.12). */
export const wcag22aa = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

/** Puts axe-core into the app window (as `window.axe`), for specs that run it from page code. */
export async function inject(): Promise<void> {
  await browser.execute(axeSource);
}

/**
 * Injects axe-core into the app window and scans the current screen, or only `context` (CSS
 * selectors, e.g. `{ include: [".pstrip"] }`). Returns the violations and writes the full report
 * to e2e/artifacts/axe-<label>.json. Asserting on the result is up to the spec.
 */
export async function scan(
  label: string,
  options: RunOptions = {},
  context?: ElementContext,
): Promise<Result[]> {
  const runOptions: RunOptions = { runOnly: { type: "tag", values: wcag22aa }, ...options };
  await inject();
  const results = await browser.execute(
    (ctx: ElementContext | null, opts: RunOptions) =>
      (
        window as unknown as {
          axe: { run: (c: ElementContext, o: RunOptions) => Promise<AxeResults> };
        }
      ).axe.run(ctx ?? document, opts),
    context ?? null,
    runOptions,
  );
  mkdirSync(artifactsDir, { recursive: true });
  writeFileSync(join(artifactsDir, `axe-${label}.json`), JSON.stringify(results, null, 2));
  return results.violations;
}

/** One line per violation, for logs and assertion messages. */
export function summarize(violations: readonly Result[]): string {
  return violations
    .map((v) => `${v.impact ?? "?"} ${v.id}: ${String(v.nodes.length)} node(s)`)
    .join("\n");
}
