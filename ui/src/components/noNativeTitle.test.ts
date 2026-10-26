import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";

/**
 * FR-2.79: tooltips are themed (`data-tooltip`), never the system's grey `title` tooltip.
 * Only an iframe keeps `title`, where it is the frame's accessible name, not a tooltip.
 */
const ALLOWED_TAGS = new Set(["iframe"]);

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return path.endsWith(".tsx") && !path.includes(".test.") ? [path] : [];
  });
}

function nativeTitles(path: string): string[] {
  const file = ts.createSourceFile(
    path,
    readFileSync(path, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const found: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxAttribute(node) && node.name.getText(file) === "title") {
      const tag = node.parent.parent.tagName.getText(file);
      // Lower-case tags are DOM elements; components may take a `title` prop of their own.
      if (/^[a-z]/.test(tag) && !ALLOWED_TAGS.has(tag)) {
        const { line } = file.getLineAndCharacterOfPosition(node.getStart(file));
        found.push(`${path}:${String(line + 1)} <${tag} title>`);
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}

describe("no native title tooltips", () => {
  it("uses data-tooltip instead of title on DOM elements", () => {
    expect(sources("ui/src").flatMap(nativeTitles)).toEqual([]);
  });
});
