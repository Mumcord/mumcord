import type { PublicServer, PublicServerPing } from "../../api";
import { serverKey } from "./model";

/** Pings waiting for their answer at the same time; the engine paces them further. */
export const PING_CONCURRENCY = 4;

type Ping = (host: string, port: number) => Promise<PublicServerPing | null>;

/**
 * Pings the servers whose rows are on screen, each once, a few at a time (FR-3.92). Rows that
 * scroll away before their turn are not pinged. The engine additionally spaces every ping out,
 * so even a fast scroll never floods anyone.
 */
export class PingScheduler {
  private visible: PublicServer[] = [];
  private readonly asked = new Set<string>();
  private running = 0;
  private stopped = false;

  constructor(
    private readonly ping: Ping,
    private readonly onResult: (key: string, answer: PublicServerPing | null) => void,
    private readonly concurrency = PING_CONCURRENCY,
  ) {}

  /** The servers on screen now, top to bottom. */
  setVisible(servers: readonly PublicServer[]): void {
    this.visible = [...servers];
    this.pump();
  }

  /** Forgets which servers were pinged, so they are asked again (Refresh). */
  reset(): void {
    this.asked.clear();
    this.pump();
  }

  /** Pings again after `stop`; the page starts it whenever it mounts. */
  start(): void {
    this.stopped = false;
    this.pump();
  }

  /** Nothing new is sent after this; answers still on their way are dropped. */
  stop(): void {
    this.stopped = true;
    this.visible = [];
  }

  private pump(): void {
    while (!this.stopped && this.running < this.concurrency) {
      const next = this.visible.find((s) => !this.asked.has(serverKey(s)));
      if (!next) return;
      const key = serverKey(next);
      this.asked.add(key);
      this.running += 1;
      this.ping(next.host, next.port)
        .catch((error: unknown) => {
          console.error("Pinging a public server failed", error);
          return null;
        })
        .then((answer) => {
          this.running -= 1;
          if (this.stopped) return;
          this.onResult(key, answer);
          this.pump();
        })
        .catch((error: unknown) => {
          console.error("Showing a public server ping failed", error);
        });
    }
  }
}
