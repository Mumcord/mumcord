// Builds the third-party notices from the licence data that scripts/generate-notices.sh collects:
// one list of entries (Rust crates, npm packages and the native components of licenses/native.json)
// with deduplicated licence texts. It writes the structured third-party-notices.json for the About
// window (issue #138) and the plain-text Rust, native and npm sections that the script joins into
// THIRD_PARTY_NOTICES, so both always list the same components. Usage:
//   node scripts/notices.js --root DIR --rust FILE --npm FILE --metadata FILE --npm-allowed IDS \
//     --out DIR
// --rust is `cargo about generate --format json`, --npm license-checker's JSON, --metadata
// `cargo metadata --format-version 1`. Fails when an npm package has a licence outside --npm-allowed.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { parseArgs } from "node:util";

const { values: opt } = parseArgs({
  options: Object.fromEntries(
    ["root", "rust", "npm", "metadata", "npm-allowed", "out"].map((k) => [k, { type: "string" }]),
  ),
});
for (const k of ["root", "rust", "npm", "metadata", "npm-allowed", "out"]) {
  if (!opt[k]) throw new Error(`--${k} missing`);
}

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const bar = "-".repeat(80);
const NO_NPM_FILE =
  "(No license file shipped with this package; see the license identifier above.)";

const texts = new Map();
/** Stores `text` once and returns its id, a hash of the content, so equal texts share one id. */
function textId(text) {
  const normal = text.replace(/\r\n?/g, "\n").replace(/\s+$/u, "");
  const id = createHash("sha256").update(normal).digest("hex").slice(0, 12);
  texts.set(id, normal);
  return id;
}

/** The SPDX identifiers in a licence expression such as "(MIT OR Apache-2.0) AND ISC". */
const spdxIds = (expr) =>
  expr
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND|WITH)\s+|\s+/u)
    .filter(Boolean);

const entries = [];

// Rust: cargo-about groups the crates by licence text. Mumcord's own workspace crates (no
// registry or git source) are not third-party and stay out.
const rust = readJson(opt.rust);
const thirdParty = (krate) => krate.source != null;
const licenses = rust.licenses
  .map((l) => ({ ...l, used_by: l.used_by.filter((u) => thirdParty(u.crate)) }))
  .filter((l) => l.used_by.length > 0);
const crates = new Map();
for (const l of licenses) {
  const id = textId(l.text);
  for (const { crate } of l.used_by) {
    const key = `${crate.name} ${crate.version}`;
    const entry = crates.get(key) ?? {
      name: crate.name,
      version: crate.version,
      license: crate.license ?? "",
      licenses: [],
      repository: crate.repository ?? null,
      source: "rust",
      textIds: [],
    };
    if (!entry.licenses.includes(l.id)) entry.licenses.push(l.id);
    if (!entry.textIds.includes(id)) entry.textIds.push(id);
    crates.set(key, entry);
  }
}
for (const entry of crates.values()) {
  if (entry.license === "") entry.license = entry.licenses.join(" AND ");
  entries.push(entry);
}

const rustOut = [
  "RUST DEPENDENCIES",
  "=================",
  "",
  "Mumcord links the following Rust crates. Each crate is listed with the",
  "license(s) it is distributed under, followed by the license text(s).",
  "",
];
for (const o of rust.overview) {
  const count = licenses.filter((l) => l.id === o.id).reduce((n, l) => n + l.used_by.length, 0);
  if (count > 0) rustOut.push(`* ${o.name} (${String(count)} crate${count > 1 ? "s" : ""})`);
}
rustOut.push("");
for (const l of licenses) {
  rustOut.push(bar, l.name, bar, "", "Used by:");
  for (const { crate } of l.used_by) {
    rustOut.push(
      `  - ${crate.name} ${crate.version}${crate.repository ? ` (${crate.repository})` : ""}`,
    );
  }
  rustOut.push("", l.text, "");
}

// npm: production packages, checked against the allowed licences. An SPDX expression is allowed
// when one OR alternative has only allowed AND parts; several detected licences (an array) must
// all be allowed.
const npm = readJson(opt.npm);
const allowed = new Set(opt["npm-allowed"].split(" "));
const ok = (expr) =>
  expr
    .replace(/[()]/g, "")
    .split(/\s+OR\s+/)
    .some((alt) => alt.split(/\s+AND\s+/).every((id) => allowed.has(id.trim())));
const packages = Object.entries(npm)
  .filter(([key]) => !key.startsWith("mumcord@"))
  .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
const rejected = packages
  .filter(([, d]) => ![].concat(d.licenses ?? "UNKNOWN").every((l) => ok(String(l))))
  .map(([key, d]) => `  ${key}: ${[].concat(d.licenses ?? "UNKNOWN").join(", ")}`);
if (rejected.length) {
  console.error(
    "error: npm packages with a licence that is not allowed (npm_allowed in scripts/generate-notices.sh):\n" +
      rejected.join("\n") +
      "\nReplace the package, or add the licence there only after checking that it is compatible with the AGPL-3.0.",
  );
  process.exit(1);
}
const npmOut = ["NPM DEPENDENCIES (production)", "=============================", ""];
for (const [key, d] of packages) {
  npmOut.push(bar, key, bar, "", `License: ${d.licenses}`);
  if (d.repository) npmOut.push(`Repository: ${d.repository}`);
  if (d.publisher) npmOut.push(`Publisher: ${d.publisher}`);
  npmOut.push("");
  let text = "";
  const f = d.licenseFile;
  if (f && existsSync(f) && !/readme/i.test(f)) text = readFileSync(f, "utf8").trim();
  npmOut.push(text || NO_NPM_FILE, "");
  const at = key.lastIndexOf("@");
  const license = [].concat(d.licenses).join(" AND ");
  entries.push({
    name: key.slice(0, at),
    version: key.slice(at + 1),
    license,
    licenses: [...new Set(spdxIds(license))],
    repository: d.repository ?? null,
    source: "npm",
    textIds: [textId(text || NO_NPM_FILE)],
  });
}

// Native components: versions come from the same build inputs the SBOM uses
// (scripts/generate-sbom.sh, scripts/build-libvpx.sh).
const metadata = readJson(opt.metadata);
const crateDir = (name) => {
  const p = metadata.packages.find((pkg) => pkg.name === name);
  if (!p) throw new Error(`${name} not found in the dependency graph`);
  return dirname(p.manifest_path);
};
const match = (file, re, what) => {
  const m = re.exec(readFileSync(file, "utf8"));
  if (!m) throw new Error(`${what} not found in ${file}`);
  return m[1];
};
const versions = {
  "@libvpx": () =>
    match(join(opt.root, "scripts/build-libvpx.sh"), /^LIBVPX_VERSION=(\S+)$/mu, "LIBVPX_VERSION"),
  "@libopus": () =>
    match(
      join(crateDir("opusic-sys"), "opus/package_version"),
      /^PACKAGE_VERSION="(.*)"$/mu,
      "the libopus version",
    ),
  "@sqlite": () =>
    match(
      join(crateDir("libsqlite3-sys"), "sqlite3/sqlite3.h"),
      /^#define SQLITE_VERSION\s+"(.*)"$/mu,
      "the SQLite version",
    ),
};
const textFiles = { "@opus-copying": () => join(crateDir("opusic-sys"), "opus/COPYING") };
const nativeOut = [];
const printed = new Map();
for (const c of readJson(join(opt.root, "licenses/native.json")).components) {
  const version = c.version.startsWith("@") ? versions[c.version]() : c.version;
  const ids = c.texts.map((t) =>
    textId(readFileSync(textFiles[t]?.() ?? join(opt.root, t), "utf8")),
  );
  const title = version ? `${c.name} ${version}` : c.name;
  nativeOut.push(bar, title, bar, "", `License: ${c.license}`, `Repository: ${c.repository}`, "");
  nativeOut.push(...c.description, "");
  // The LGPL is shared by several components; its text is printed once.
  for (const id of ids) {
    nativeOut.push(
      printed.has(id) ? `(License text: see ${printed.get(id)} above.)` : texts.get(id),
    );
    nativeOut.push("");
    if (!printed.has(id)) printed.set(id, c.name);
  }
  entries.push({
    name: c.name,
    version,
    license: c.license,
    licenses: [...new Set(spdxIds(c.license))],
    repository: c.repository,
    source: "native",
    textIds: ids,
    description: c.description.join(" "),
  });
}

entries.sort(
  (a, b) =>
    a.name.localeCompare(b.name, "en", { sensitivity: "base" }) ||
    a.version.localeCompare(b.version, "en", { numeric: true }) ||
    a.source.localeCompare(b.source),
);
const used = new Set(entries.flatMap((e) => e.textIds));
const sortedTexts = Object.fromEntries([...texts].filter(([id]) => used.has(id)).sort());

const write = (name, lines) => {
  writeFileSync(join(opt.out, name), lines.join("\n") + "\n");
};
write("rust.txt", rustOut);
write("native.txt", nativeOut);
write("npm.txt", npmOut);
writeFileSync(
  join(opt.out, "third-party-notices.json"),
  JSON.stringify({ entries, texts: sortedTexts }) + "\n",
);
