// Splits the E2E spec files into the CI shards of e2e/shards.json, balanced by how long each spec
// takes in CI (e2e/spec-durations.json), so that every shard job needs about the same wall time.
//   node scripts/e2e-shards.js [--shards N]       rebalance shards.json from the durations file
//   node scripts/e2e-shards.js --measure RUN_ID…  first update the durations from E2E runs (gh CLI)
// The network resilience spec is pinned alone to shard 1: it is the longest spec and needs the
// netsim proxy for its whole run. The other specs are spread longest-first, each onto the shard
// with the least time so far. A spec without a measured duration counts with the median.
import { execFileSync } from "node:child_process";
import { globSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..");
const e2eDir = join(repo, "e2e");
const durationsFile = join(e2eDir, "spec-durations.json");
const shardsFile = join(e2eDir, "shards.json");
const workflowFile = join(repo, ".github/workflows/e2e.yml");
const pinned = "specs/network/network-resilience.e2e.ts";
// Colour codes in the job logs, which "gh api --allow-escape-sequences" keeps.
const ansiColour = new RegExp(String.fromCharCode(27) + "\\[[0-9;]*m", "g");

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
 * Seconds per spec file from the shard logs of the given workflow runs: from a spec's "RUNNING"
 * line to its "PASSED" line, plus the ~1 s until the next spec starts. Failed specs are skipped.
 */
function measure(ids) {
  const samples = new Map();
  for (const id of ids) {
    const jobs = JSON.parse(
      execFileSync("gh", ["run", "view", id, "--json", "jobs"], { encoding: "utf8" }),
    ).jobs.filter((j) => j.name.startsWith("E2E shard") && j.conclusion === "success");
    for (const job of jobs) {
      const log = execFileSync(
        "gh",
        [
          "api",
          "--allow-escape-sequences",
          `repos/{owner}/{repo}/actions/jobs/${job.databaseId}/logs`,
        ],
        { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
      );
      const started = new Map();
      for (const line of log.split("\n")) {
        const m = /^(\S+Z) .*(RUNNING|PASSED) in .*\/e2e\/(specs\/\S+\.e2e\.ts)/.exec(
          line.replace(ansiColour, ""),
        );
        if (!m) continue;
        const [, time, state, spec] = m;
        if (state === "RUNNING") started.set(spec, Date.parse(time));
        else if (started.has(spec)) {
          const s = (Date.parse(time) - started.get(spec)) / 1000 + 1.25;
          samples.set(spec, [...(samples.get(spec) ?? []), s]);
        }
      }
    }
  }
  if (samples.size === 0) throw new Error("no passed spec found in the shard logs of these runs");
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

// Longest-processing-time-first: each spec goes to the shard with the least time so far.
const rest = Array.from({ length: count - 1 }, () => ({ total: 0, specs: [] }));
for (const spec of specs.filter((s) => s !== pinned).sort((a, b) => cost(b) - cost(a))) {
  const shard = rest.reduce((min, s) => (s.total < min.total ? s : min));
  shard.specs.push(spec);
  shard.total += cost(spec);
}
rest.sort((a, b) => b.total - a.total);
const shards = [[pinned], ...rest.map((s) => s.specs.sort())];
writeFileSync(shardsFile, JSON.stringify({ shards }, null, 2) + "\n");
// The repository formats JSON with Prettier (npm run lint checks it).
execFileSync("npx", ["prettier", "--write", shardsFile, durationsFile], { stdio: "ignore" });

[cost(pinned), ...rest.map((s) => s.total)].forEach((t, i) => {
  console.log(
    `shard ${String(i + 1)}: ${String(Math.round(t))} s, ${String(shards[i].length)} specs`,
  );
});
const matrix = /shard: \[([\d, ]+)\]/.exec(readFileSync(workflowFile, "utf8"));
if (matrix?.[1].split(",").length !== count) {
  console.warn(`update the shard matrix in .github/workflows/e2e.yml to 1..${String(count)}`);
}
