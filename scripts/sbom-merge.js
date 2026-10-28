// Merges the per-tool CycloneDX SBOMs of one platform into a single file (scripts/generate-sbom.sh
// calls it). The first Rust SBOM (the app crate) becomes the root; the other Rust SBOMs, the npm
// SBOM of the UI and the native libraries are added as components the root depends on.
//   node scripts/sbom-merge.js --version V --platform P --root DIR --native FILE --gstreamer gst|-
//     --opus V --npm FILE --out FILE app.json [more-rust.json ...]
import { randomUUID } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import process from "node:process";
import { parseArgs } from "node:util";

const { values: opt, positionals: rustFiles } = parseArgs({
  allowPositionals: true,
  options: Object.fromEntries(
    ["version", "platform", "root", "native", "gstreamer", "opus", "npm", "out"].map((k) => [
      k,
      { type: "string" },
    ]),
  ),
});
for (const [k, v] of Object.entries(opt)) if (!v) throw new Error(`--${k} is required`);
if (rustFiles.length === 0) throw new Error("at least one Rust SBOM is required");

// cargo-cyclonedx writes absolute paths of the build checkout into the refs of workspace crates.
const load = (file) =>
  JSON.parse(readFileSync(file, "utf8").replaceAll(`file://${opt.root}`, "file://"));
const [app, ...extraRust] = rustFiles.map(load);
const npm = load(opt.npm);

const components = new Map();
const dependencies = new Map();
const add = (c) => {
  if (!c["bom-ref"]) throw new Error(`component without bom-ref: ${c.name}`);
  if (!components.has(c["bom-ref"])) components.set(c["bom-ref"], c);
};
const addDeps = (bom) => {
  for (const d of bom.dependencies ?? []) {
    const known = dependencies.get(d.ref) ?? new Set();
    for (const on of d.dependsOn ?? []) known.add(on);
    dependencies.set(d.ref, known);
  }
};

const root = app.metadata.component;
root.version = opt.version;
if (root.purl) root.purl = root.purl.replace(/@[^?]*/, `@${opt.version}`);
const rootDeps = [];
for (const c of app.components ?? []) add(c);
addDeps(app);
for (const bom of [...extraRust, npm]) {
  add(bom.metadata.component);
  rootDeps.push(bom.metadata.component["bom-ref"]);
  for (const c of bom.components ?? []) add(c);
  addDeps(bom);
}

const native = (name, version, purl, licence, description, group) => {
  const ref = `native:${group ? `${group}/` : ""}${name}@${version}`;
  add({
    type: "library",
    "bom-ref": ref,
    ...(group && { group }),
    name,
    version,
    description,
    licenses: [{ license: { id: licence } }],
    ...(purl && { purl }),
  });
  rootDeps.push(ref);
};

native(
  "libopus",
  opt.opus,
  `pkg:github/xiph/opus@v${opt.opus}`,
  "BSD-3-Clause",
  "Opus voice codec, compiled from the source vendored by the opusic-sys crate.",
);
const lines = readFileSync(opt.native, "utf8")
  .split("\n")
  .filter(Boolean)
  .map((l) => l.split(" "));
const field = (key) => lines.filter(([k]) => k === key).map(([, v]) => v);
const [libvpx] = field("libvpx");
if (!libvpx) throw new Error(`${opt.native} has no libvpx line`);
native(
  "libvpx",
  libvpx,
  `pkg:github/webmproject/libvpx@v${libvpx}`,
  "BSD-3-Clause",
  "VP8/VP9 screen-share codec, statically linked.",
);
if (opt.gstreamer === "gst") {
  const [webkit] = field("webkitgtk");
  if (!webkit) throw new Error(`${opt.native} has no WebKitGTK line`);
  native(
    "webkitgtk",
    webkit,
    `pkg:deb/ubuntu/libwebkit2gtk-4.1-0@${webkit}`,
    "LGPL-2.1-or-later",
    "WebKitGTK web view from the Ubuntu 24.04 build host, bundled in the AppImage.",
  );
  const [gst] = field("gstreamer");
  const plugins = field("gst-plugin");
  if (!gst || plugins.length === 0) throw new Error(`${opt.native} has no GStreamer lines`);
  native(
    "gstreamer",
    gst,
    `pkg:deb/ubuntu/libgstreamer1.0-0@${gst}`,
    "LGPL-2.1-or-later",
    "GStreamer core libraries from the Ubuntu 24.04 build host, bundled in the AppImage.",
  );
  for (const p of plugins) {
    native(
      p,
      gst,
      null,
      "LGPL-2.1-or-later",
      "GStreamer plugin bundled in the AppImage.",
      "gstreamer",
    );
  }
}

dependencies.set(
  root["bom-ref"],
  new Set([...(dependencies.get(root["bom-ref"]) ?? []), ...rootDeps]),
);
// cargo-cyclonedx writes the legacy tool list ({vendor, name, version}); spec 1.5 wants components.
const tools = [
  ...(app.metadata.tools?.components ?? app.metadata.tools ?? []).map(({ name, version }) => ({
    type: "application",
    name,
    version,
  })),
  ...(npm.metadata.tools?.components ?? []),
  { type: "application", name: "scripts/sbom-merge.js", version: opt.version },
];

const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  serialNumber: `urn:uuid:${randomUUID()}`,
  version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: { components: tools },
    component: root,
    properties: [{ name: "mumcord:platform", value: opt.platform }],
  },
  components: [...components.values()],
  dependencies: [...dependencies].map(([ref, on]) => ({ ref, dependsOn: [...on] })),
};

const refs = new Set([root["bom-ref"], ...components.keys()]);
const dangling = bom.dependencies
  .flatMap((d) => [d.ref, ...d.dependsOn])
  .filter((r) => !refs.has(r));
if (dangling.length)
  throw new Error(`dependencies refer to unknown components: ${[...new Set(dangling)].join(", ")}`);

writeFileSync(opt.out, JSON.stringify(bom, null, 2) + "\n");
process.stdout.write(`${opt.platform}: ${components.size} components\n`);
