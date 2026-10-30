import { describe, expect, it, vi } from "vitest";
import type { PublicServer, PublicServerPing } from "../../api";
import { serverKey } from "./model";
import { PingScheduler } from "./pingScheduler";

const server = (i: number): PublicServer => ({
  name: `S${String(i)}`,
  host: `s${String(i)}.example.org`,
  port: 64738,
  country: "",
  countryCode: "",
});
const servers = Array.from({ length: 10 }, (_, i) => server(i));

/** A ping whose answers the test releases by hand. */
function manualPing() {
  const pending: { host: string; resolve: (answer: PublicServerPing | null) => void }[] = [];
  const ping = vi.fn(
    (host: string) =>
      new Promise<PublicServerPing | null>((resolve) => {
        pending.push({ host, resolve });
      }),
  );
  const answerNext = async () => {
    pending.shift()?.resolve({ pingMs: 10, users: 1, maxUsers: 5 });
    await Promise.resolve();
    await Promise.resolve();
  };
  return { ping, pending, answerNext };
}

describe("PingScheduler", () => {
  it("pings only visible servers, at most a few at a time", async () => {
    const { ping, answerNext } = manualPing();
    const results: string[] = [];
    const scheduler = new PingScheduler(ping, (key) => results.push(key), 2);
    scheduler.setVisible(servers.slice(0, 5));
    expect(ping).toHaveBeenCalledTimes(2);
    await answerNext();
    expect(ping).toHaveBeenCalledTimes(3);
    expect(results).toEqual([serverKey(server(0))]);
    expect(ping.mock.calls.map((c) => c[0])).not.toContain(server(5).host);
  });

  it("skips rows that scrolled away before their turn and never pings twice", async () => {
    const { ping, answerNext } = manualPing();
    const scheduler = new PingScheduler(ping, () => undefined, 1);
    scheduler.setVisible(servers.slice(0, 3));
    scheduler.setVisible(servers.slice(5, 7));
    await answerNext();
    await answerNext();
    await answerNext();
    expect(ping.mock.calls.map((c) => c[0])).toEqual([
      server(0).host,
      server(5).host,
      server(6).host,
    ]);
    scheduler.setVisible(servers.slice(5, 7));
    expect(ping).toHaveBeenCalledTimes(3);
  });

  it("asks again after a reset and stops for good", async () => {
    const { ping, answerNext } = manualPing();
    const onResult = vi.fn();
    const scheduler = new PingScheduler(ping, onResult, 1);
    scheduler.setVisible(servers.slice(0, 1));
    await answerNext();
    scheduler.reset();
    expect(ping).toHaveBeenCalledTimes(2);
    scheduler.stop();
    await answerNext();
    expect(onResult).toHaveBeenCalledTimes(1);
    scheduler.setVisible(servers);
    expect(ping).toHaveBeenCalledTimes(2);
  });
});
