#!/bin/bash
# Tests the third-party notices (scripts/generate-notices.sh, scripts/notices.js, issue #138):
# the committed ui/public/third-party-notices.json is valid, every entry has its licence texts and
# every text is used once under the hash of its content; the plain-text notices list the same
# components per source; and on small inputs equal licence texts share one id and an npm package
# with a licence that is not allowed fails the run.
#   scripts/ci/test-notices.sh
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cmp -s "$repo/THIRD_PARTY_NOTICES" "$repo/ui/public/third-party-notices.txt" \
  || { echo "FAIL: THIRD_PARTY_NOTICES and ui/public/third-party-notices.txt differ" >&2; exit 1; }

node - "$repo/ui/public/third-party-notices.json" "$repo/THIRD_PARTY_NOTICES" <<'NODE'
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const fail = (message) => {
  console.error(`FAIL: ${message}`);
  process.exit(1);
};
const data = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const text = fs.readFileSync(process.argv[3], "utf8");
if (!Array.isArray(data.entries) || data.entries.length === 0) fail("no entries");
const used = new Set();
for (const e of data.entries) {
  const what = `${e.source} entry ${e.name} ${e.version}`;
  if (!["rust", "npm", "native"].includes(e.source)) fail(`${what}: unknown source`);
  if (!e.name || typeof e.version !== "string" || !e.license) fail(`${what}: incomplete`);
  if (!Array.isArray(e.licenses) || e.licenses.length === 0) fail(`${what}: no licence ids`);
  if (!Array.isArray(e.textIds) || e.textIds.length === 0) fail(`${what}: no licence text`);
  for (const id of e.textIds) {
    if (!data.texts[id]) fail(`${what}: text ${id} missing`);
    used.add(id);
  }
}
const seen = new Set();
for (const [id, t] of Object.entries(data.texts)) {
  if (!used.has(id)) fail(`text ${id} is not used by any entry`);
  if (createHash("sha256").update(t).digest("hex").slice(0, 12) !== id) fail(`text ${id}: id is not its hash`);
  if (seen.has(t)) fail(`text ${id} is stored twice`);
  seen.add(t);
}

// The plain text has one "Used by" line per crate and licence, and a header block per npm package
// and native component.
const section = (start, end) => text.slice(text.indexOf(start), end ? text.indexOf(end) : undefined);
const headers = (s) => [...s.matchAll(/^-{80}\n(.+)\n-{80}$/gmu)].map((m) => m[1]);
const rust = new Set(
  [...section("RUST DEPENDENCIES\n", "NPM DEPENDENCIES").matchAll(/^ {2}- (\S+ \S+)/gmu)].map((m) => m[1]),
);
const counts = {
  rust: rust.size,
  npm: headers(section("NPM DEPENDENCIES")).length,
  native: headers(section("BUNDLED COMPONENTS", "OTHER LINUX SYSTEM LIBRARIES")).length,
};
for (const [source, n] of Object.entries(counts)) {
  const json = data.entries.filter((e) => e.source === source).length;
  if (json !== n) fail(`${source}: ${json} entries in the JSON, ${n} in the plain text`);
}
console.log(`ok: ${JSON.stringify(counts)}, ${Object.keys(data.texts).length} texts`);
NODE

# Small inputs: two crates and two npm packages, all with the same MIT text.
mit=$'MIT License\n\nPermission is hereby granted, free of charge.'
mkdir -p "$work/opus/opus" "$work/sqlite/sqlite3" "$work/npm/a" "$work/npm/b" "$work/out"
echo 'PACKAGE_VERSION="1.0.0"' > "$work/opus/opus/package_version"
echo "Opus licence" > "$work/opus/opus/COPYING"
echo '#define SQLITE_VERSION        "3.0.0"' > "$work/sqlite/sqlite3/sqlite3.h"
printf '%s\n' "$mit" > "$work/npm/a/LICENSE"
printf '%s\r\n' "$mit" > "$work/npm/b/LICENSE"
cat > "$work/metadata.json" <<JSON
{"packages": [{"name": "opusic-sys", "manifest_path": "$work/opus/Cargo.toml"},
              {"name": "libsqlite3-sys", "manifest_path": "$work/sqlite/Cargo.toml"}]}
JSON
node - "$mit" "$work" <<'NODE'
const [mit, out] = process.argv.slice(2);
const crate = (name, source) => ({ crate: { name, version: "1.0.0", source, license: "MIT" } });
const rust = {
  overview: [{ id: "MIT", name: "MIT License" }],
  licenses: [
    { id: "MIT", name: "MIT License", text: mit, used_by: [crate("alpha", "registry"), crate("mumcord", null)] },
    { id: "MIT", name: "MIT License", text: mit + "\n", used_by: [crate("beta", "registry")] },
  ],
};
require("node:fs").writeFileSync(`${out}/rust.json`, JSON.stringify(rust));
NODE
npm() {
  cat > "$work/npm.json" <<JSON
{"a@1.0.0": {"licenses": "$1", "licenseFile": "$work/npm/a/LICENSE"},
 "b@2.0.0": {"licenses": "MIT", "licenseFile": "$work/npm/b/LICENSE"},
 "mumcord@0.0.0": {"licenses": "AGPL-3.0-only"}}
JSON
  node "$repo/scripts/notices.js" --root "$repo" --rust "$work/rust.json" --npm "$work/npm.json" \
    --metadata "$work/metadata.json" --npm-allowed "MIT ISC" --out "$work/out"
}
npm "(MIT OR GPL-3.0-only)"
node - "$work/out/third-party-notices.json" <<'NODE'
const data = JSON.parse(require("node:fs").readFileSync(process.argv[2], "utf8"));
const fail = (message) => {
  console.error(`FAIL: ${message}`);
  process.exit(1);
};
const thirdParty = data.entries.filter((e) => e.source !== "native");
const names = thirdParty.map((e) => e.name).join(" ");
if (names !== "a alpha b beta") fail(`unexpected entries: ${names}`);
const ids = new Set(thirdParty.flatMap((e) => e.textIds));
if (ids.size !== 1) fail(`equal MIT texts got ${ids.size} ids`);
const a = thirdParty.find((e) => e.name === "a");
if (a.licenses.join(" ") !== "MIT GPL-3.0-only") fail(`licence ids of a: ${a.licenses.join(" ")}`);
const opus = data.entries.find((e) => e.name === "libopus");
if (opus.version !== "1.0.0" || data.texts[opus.textIds[0]] !== "Opus licence") fail("libopus");
NODE

if npm "GPL-3.0-only" 2> "$work/err.txt"; then
  echo "FAIL: an npm package under GPL-3.0-only was accepted" >&2
  exit 1
fi
grep -q "a@1.0.0: GPL-3.0-only" "$work/err.txt" || { echo "FAIL: the rejection does not name the package" >&2; exit 1; }
echo "ok: notices"
