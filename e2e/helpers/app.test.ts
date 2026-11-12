// @vitest-environment node
import { EventEmitter } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { instance, startDriver } from "./app.ts";

const mocks = vi.hoisted(() => ({
  openPorts: new Set<number>(),
  driver: { kill: vi.fn() },
}));

vi.mock("@wdio/globals", () => ({ browser: {} }));
vi.mock("node:child_process", () => ({ spawn: vi.fn(() => mocks.driver) }));
vi.mock("node:net", () => ({
  createConnection: ({ port }: { port: number }) => {
    const socket = Object.assign(new EventEmitter(), { end: vi.fn() });
    void Promise.resolve().then(() => {
      socket.emit(mocks.openPorts.has(port) ? "connect" : "error");
    });
    return socket;
  },
}));

beforeEach(() => {
  vi.useFakeTimers();
  mocks.openPorts.clear();
  // Skip profile/keyring creation: these tests only exercise driver readiness.
  instance("a").profileDir = "/tmp/mumcord-driver-test";
});

afterEach(() => {
  instance("a").driver = undefined;
  instance("a").profileDir = "";
  vi.useRealTimers();
});

describe("startDriver", () => {
  it.each(["proxy", "native"] as const)(
    "waits for the %s port before starting a session",
    async (delayed) => {
      const proxyPort = instance("a").port;
      const nativePort = proxyPort + 1;
      const delayedPort = delayed === "proxy" ? proxyPort : nativePort;
      mocks.openPorts.add(delayed === "proxy" ? nativePort : proxyPort);
      let ready = false;
      const starting = startDriver().then(() => {
        ready = true;
      });

      await vi.advanceTimersByTimeAsync(0);
      expect(ready).toBe(false);

      mocks.openPorts.add(delayedPort);
      await vi.advanceTimersByTimeAsync(100);
      await starting;
      expect(ready).toBe(true);
    },
  );

  it("fails clearly when the native driver never opens its port", async () => {
    const proxyPort = instance("a").port;
    mocks.openPorts.add(proxyPort);
    const failure = expect(startDriver()).rejects.toThrow(
      `port ${String(proxyPort + 1)} did not open`,
    );

    await Promise.all([failure, vi.advanceTimersByTimeAsync(10_000)]);
  });
});
