import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { CONTROL_GAP, CONTROL_ROW, EXTRA, WIDGET_BORDER } from "./WatchPartyWidget";

const css = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");

function rule(source: string, selector: string): string {
  const start = source.indexOf(`${selector} {`);
  expect(start).toBeGreaterThanOrEqual(0);
  return source.slice(start, source.indexOf("}", start));
}

describe("watch party widget extra height", () => {
  it("equals the declared CSS sizes of the control row, gap and border", () => {
    const header = rule(
      css("./watchPartyWidget.css"),
      ".watch-party-widget .watch-playlist__header",
    );
    expect(header).toContain(`height: ${String(CONTROL_ROW)}px`);
    expect(header).toContain(`margin-top: var(--space-1)`);
    const widget = rule(css("../../components/floatingWidget.css"), ".floating-widget");
    expect(widget).toContain("border: 1px solid");
    expect(EXTRA).toBe(CONTROL_ROW + CONTROL_GAP + WIDGET_BORDER);
  });
});
