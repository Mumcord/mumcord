// @vitest-environment node
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";
import { saveFailureLogs } from "./failure-logs.ts";

const dirs: string[] = [];
function temporary(): string {
  const dir = mkdtempSync(join(tmpdir(), "mumcord-failure-logs-"));
  dirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

it("copies only diagnostic log files, labelled by the failing client", () => {
  const profile = temporary();
  const destination = join(temporary(), "artifacts");
  const logs = join(profile, "data", "app.mumcord.client", "logs");
  mkdirSync(logs, { recursive: true });
  writeFileSync(join(logs, "mumcord.2026-11-13.log"), "camera stopped reason=device-gone\n");
  writeFileSync(join(logs, "identity.p12"), "excluded fixture");
  writeFileSync(join(logs, "settings.sqlite"), "excluded fixture");
  symlinkSync(join(logs, "identity.p12"), join(logs, "mumcord.2026-11-12.log"));
  mkdirSync(join(logs, "mumcord.2026-11-11.log"));

  saveFailureLogs(profile, destination, "camera-notice-C");
  expect(readdirSync(destination)).toEqual(["camera-notice-C-mumcord.2026-11-13.log"]);
  expect(readFileSync(join(destination, "camera-notice-C-mumcord.2026-11-13.log"), "utf8")).toBe(
    "camera stopped reason=device-gone\n",
  );
});

it("tolerates a client that failed before its log directory was created", () => {
  const profile = temporary();
  const destination = temporary();
  saveFailureLogs(profile, destination, "startup-A");
  expect(readdirSync(destination)).toEqual([]);
});
