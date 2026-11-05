// Splits the E2E spec files into the CI shards of e2e/shards.json, balanced by how long each spec
// takes in CI (e2e/spec-durations.json), so that every shard job needs about the same wall time.
//   node scripts/e2e-shards.js [--shards N]       rebalance shards.json from the durations file
//   node scripts/e2e-shards.js --measure RUN_ID…  first update the durations from E2E runs (gh CLI)
// The network resilience spec is pinned alone to shard 1: it is the longest spec and needs the
// netsim proxy for its whole run. The relay specs (the ones that start the mumpeer testenv) go to
// as few shards as the balance allows, because only those shards load the relay images, and run
// first there, ordered by relay profile, so they share one environment (e2e/helpers/relayenv.ts).
// The other specs are spread longest-first, each onto the shard with the least time so far. A
// spec without a measured duration counts with the median.
import { execFileSync } from "node:child_process";
import { globSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const e2eDir = join(repo, "e2e");
const durationsFile = join(e2eDir, "spec-durations.json");
const shardsFile = join(e2eDir, "shards.json");
const workflowFile = join(repo, ".github/workflows/e2e.yml");
const pinned = "specs/network/network-resilience.e2e.ts";
// Loading the relay images and the first start of the relay environment: a relay shard's cost
// that is in no spec's measured time.
const relaySetup = 20;

const { values: opt, positionals: runIds } = parseArgs({
  allowPositionals: true,
  options: { shards: { type: "string" }, measure: { type: "boolean" } },
});

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

/**
 * Seconds per spec file from the "e2e-durations-shard-N" artifacts of the given workflow runs
 * (wdio.conf.ts writes them; only passed spec files are in there). The median per spec is kept.
 */
function measure(ids) {
  const samples = new Map();
  const tmp = mkdtempSync(join(tmpdir(), "e2e-durations-"));
  try {
    for (const id of ids) {
      const dir = join(tmp, id);
      execFileSync("gh", ["run", "download", id, "--pattern", "e2e-durations-*", "--dir", dir], {
        stdio: ["ignore", "ignore", "inherit"],
      });
      for (const file of globSync("*/spec-durations.json", { cwd: dir })) {
        const { seconds } = JSON.parse(readFileSync(join(dir, file), "utf8"));
        for (const [spec, s] of Object.entries(seconds)) {
          samples.set(spec, [...(samples.get(spec) ?? []), s]);
        }
      }
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  if (samples.size === 0) throw new Error("no spec durations in the artifacts of these runs");
  const seconds = Object.fromEntries(
    [...samples]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => [k, Math.round(median(v) * 10) / 10]),
  );
  const note = "Median CI seconds per E2E spec file; written by scripts/e2e-shards.js --measure.";
  writeFileSync(durationsFile, JSON.stringify({ note, seconds }, null, 2) + "\n");
  console.log(`measured ${String(samples.size)} spec files from ${String(ids.length)} runs`);
}

if (opt.measure) {
  if (runIds.length === 0) throw new Error("--measure needs at least one workflow run id");
  measure(runIds);
}

const current = JSON.parse(readFileSync(shardsFile, "utf8")).shards;
const count = Number(opt.shards ?? current.length);
if (!Number.isInteger(count) || count < 2) throw new Error("--shards must be an integer ≥ 2");

const { seconds } = JSON.parse(readFileSync(durationsFile, "utf8"));
const specs = globSync("specs/**/*.e2e.ts", { cwd: e2eDir }).sort();
if (!specs.includes(pinned)) throw new Error(`${pinned} is missing; update the pinned spec`);
const fallback = median(Object.values(seconds));
const unmeasured = specs.filter((s) => !(s in seconds));
if (unmeasured.length > 0) {
  console.warn(`no duration for ${unmeasured.join(", ")}; counted as ${String(fallback)} s`);
}
const cost = (s) => seconds[s] ?? fallback;

/** Whether a spec starts the mumpeer relay environment, and with which profile first. */
const profiles = new Map(
  specs.map((s) => [
    s,
    /relayenv\.startRelay\(\s*"([\w-]+)"/.exec(readFileSync(join(e2eDir, s), "utf8"))?.[1],
  ]),
);
const firstProfile = (spec) => profiles.get(spec);

/** Longest-processing-time-first: each spec goes to the shard with the least time so far. */
function spread(list, shards) {
  for (const spec of [...list].sort((a, b) => cost(b) - cost(a))) {
    const shard = shards.reduce((min, s) => (s.total < min.total ? s : min));
    shard.specs.push(spec);
    shard.total += cost(spec);
  }
  return shards;
}
const empty = (n, total = 0) => Array.from({ length: n }, () => ({ total, specs: [] }));

const others = specs.filter((s) => s !== pinned);
const relaySpecs = others.filter((s) => firstProfile(s) !== undefined);
const target = Math.max(
  ...others.map(cost),
  (others.reduce((sum, s) => sum + cost(s), 0) + relaySetup) / (count - 1),
);
// The fewest relay shards whose relay specs stay within the target time of a shard.
let relayShards = [];
for (let k = 1; k <= count - 1; k++) {
  relayShards = spread(relaySpecs, empty(k, relaySetup));
  if (Math.max(...relayShards.map((s) => s.total)) <= target) break;
}
const rest = spread(
  others.filter((s) => !relaySpecs.includes(s)),
  [...relayShards, ...empty(count - 1 - relayShards.length)],
);
rest.sort((a, b) => b.total - a.total);
// Relay specs first, by profile, then the others; wdio runs a shard's files in this order.
const order = (a, b) =>
  Number(firstProfile(a) === undefined) - Number(firstProfile(b) === undefined) ||
  (firstProfile(a) ?? "").localeCompare(firstProfile(b) ?? "") ||
  a.localeCompare(b);
const shards = [[pinned], ...rest.map((s) => s.specs.sort(order))];
writeFileSync(shardsFile, JSON.stringify({ shards }, null, 2) + "\n");
// The repository formats JSON with Prettier (npm run lint checks it).
execFileSync("npx", ["prettier", "--write", shardsFile, durationsFile], { stdio: "ignore" });

[cost(pinned), ...rest.map((s) => s.total)].forEach((t, i) => {
  const relay = shards[i].some((s) => firstProfile(s) !== undefined) ? ", relay" : "";
  console.log(
    `shard ${String(i + 1)}: ${String(Math.round(t))} s, ${String(shards[i].length)} specs${relay}`,
  );
});
const matrix = /shard: \[([\d, ]+)\]/.exec(readFileSync(workflowFile, "utf8"));
if (matrix?.[1].split(",").length !== count) {
  console.warn(`update the shard matrix in .github/workflows/e2e.yml to 1..${String(count)}`);
}
