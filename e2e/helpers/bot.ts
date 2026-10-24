import { spawn, type ChildProcess } from "node:child_process";
import { bin } from "./paths.ts";
import { run, type RunResult } from "./run.ts";

export interface BotOptions {
  name?: string;
  /** 64738 = v15 (default), 64739 = v14, 64740 = v13, 64741 = limit, 64800 = via netsim. */
  port?: number;
  cert?: string;
  superuser?: boolean;
  /** Seconds for login and each wait (CLI default 10). */
  timeout?: number;
  /** Stay connected this many seconds after the command. */
  hold?: number;
}

function globalArgs(opts: BotOptions): string[] {
  const args: string[] = [];
  if (opts.name !== undefined) args.push("--name", opts.name);
  if (opts.port !== undefined) args.push("--port", String(opts.port));
  if (opts.cert !== undefined) args.push("--cert", opts.cert);
  if (opts.superuser) args.push("--superuser");
  if (opts.timeout !== undefined) args.push("--timeout", String(opts.timeout));
  if (opts.hold !== undefined) args.push("--hold", String(opts.hold));
  return args;
}

/**
 * Runs one `mumcord-bot` subcommand, e.g. `bot(["expect", "user", "Tester"])`, and throws unless it
 * exits 0 (exit 0 = check passed). See `mumcord-bot --help` for subcommands.
 */
export async function bot(command: readonly string[], opts: BotOptions = {}): Promise<RunResult> {
  const result = await run(bin.bot, [...globalArgs(opts), ...command]);
  if (result.code !== 0) {
    throw new Error(
      `mumcord-bot ${command.join(" ")} failed (${String(result.code)}):\n${result.stderr}`,
    );
  }
  return result;
}

/** A bot running in the background (e.g. `script` fed via stdin, or a long `--hold`). */
export interface BackgroundBot {
  process: ChildProcess;
  /** Resolves with the exit result; rejects if it exits non-zero. */
  done: Promise<RunResult>;
  /** Writes one `script` line to the bot's stdin. */
  send(line: string): void;
  stop(): void;
}

export function botInBackground(command: readonly string[], opts: BotOptions = {}): BackgroundBot {
  const child = spawn(bin.bot, [...globalArgs(opts), ...command], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
  child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
  const done = new Promise<RunResult>((resolvePromise, reject) => {
    child.on("error", reject);
    child.on("close", (code) => {
      const result = { code: code ?? -1, stdout, stderr };
      if (code === 0) resolvePromise(result);
      else
        reject(new Error(`mumcord-bot ${command.join(" ")} failed (${String(code)}):\n${stderr}`));
    });
  });
  return {
    process: child,
    done,
    send: (line) => child.stdin.write(`${line}\n`),
    stop: () => child.kill("SIGTERM"),
  };
}
