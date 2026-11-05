// Writes crates/plugins/assets/builtin-labels.json: the normalised built-in menu and palette
// labels of all locales that plugins may not reuse (FR-9.54). The key list is
// scripts/builtin-label-keys.json. A Rust test checks that the committed file matches.
// Normalisation must match `normalize_label` in crates/plugins/src/manifest.rs.
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const locales = join(root, "ui/src/locales");

function normalize(label) {
  return label
    .toLowerCase()
    .replace(/\s+/gu, "")
    .replace(/(…|\.\.\.)$/u, "");
}

function lookup(object, path) {
  return path.split(".").reduce((value, part) => value?.[part], object);
}

const { keys } = JSON.parse(readFileSync(join(root, "scripts/builtin-label-keys.json"), "utf8"));
const labels = new Set();
for (const locale of readdirSync(locales).sort()) {
  for (const { namespace, key } of keys) {
    const file = join(locales, locale, `${namespace}.json`);
    const value = lookup(JSON.parse(readFileSync(file, "utf8")), key);
    if (typeof value !== "string") throw new Error(`${locale}/${namespace}.json lacks ${key}`);
    labels.add(normalize(value));
  }
}
const sorted = [...labels].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
writeFileSync(
  join(root, "crates/plugins/assets/builtin-labels.json"),
  `${JSON.stringify(sorted, null, 2)}\n`,
);
