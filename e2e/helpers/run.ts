import { spawn } from "node:child_process";

export interface RunResult {
  code: number;
  stdout: string;
  stderr: string;
}

/** Runs a command to completion and collects its output; never throws on a non-zero exit. */
export function run(
  cmd: string,
  args: readonly string[],
  env?: NodeJS.ProcessEnv,
): Promise<RunResult> {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cmd, args, { env: { ...process.env, ...env } });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
    child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => {
      resolvePromise({ code: code ?? -1, stdout, stderr });
    });
  });
}

/** Like `run`, but throws with the command's output when it exits non-zero. */
export async function runOk(
  cmd: string,
  args: readonly string[],
  env?: NodeJS.ProcessEnv,
): Promise<RunResult> {
  const result = await run(cmd, args, env);
  if (result.code !== 0) {
    throw new Error(
      `${cmd} ${args.join(" ")} exited ${String(result.code)}\n${result.stdout}${result.stderr}`,
    );
  }
  return result;
}
