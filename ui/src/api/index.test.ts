import { afterEach, describe, expect, it } from "vitest";
import {
  createMockApi,
  getApi,
  selectApiKind,
  setApi,
  toEngineError,
  type ApiEnvironment,
} from ".";

const prod: ApiEnvironment = {
  dev: false,
  mode: "production",
  forceMock: undefined,
  inTauri: false,
};

describe("selectApiKind", () => {
  it("uses the real engine inside Tauri", () => {
    expect(selectApiKind({ ...prod, inTauri: true })).toBe("tauri");
    expect(selectApiKind({ ...prod, dev: true, inTauri: true })).toBe("tauri");
  });

  it("falls back to the mock outside Tauri only in dev and test builds", () => {
    expect(selectApiKind({ ...prod, dev: true })).toBe("mock");
    expect(selectApiKind({ ...prod, mode: "test" })).toBe("mock");
  });

  it("fails loudly in a production build outside Tauri", () => {
    expect(() => selectApiKind(prod)).toThrow(/engine unavailable/);
  });

  it("uses the mock when forced, even in production or inside Tauri", () => {
    expect(selectApiKind({ ...prod, forceMock: "1" })).toBe("mock");
    expect(selectApiKind({ ...prod, forceMock: "1", inTauri: true })).toBe("mock");
  });
});

describe("getApi", () => {
  afterEach(() => {
    setApi(undefined);
  });

  it("uses the mock under vitest and allows injection", () => {
    expect(getApi().kind).toBe("mock");
    const injected = createMockApi({ seed: "fresh" });
    setApi(injected);
    expect(getApi()).toBe(injected);
  });
});

describe("toEngineError", () => {
  it("normalises engine errors, framework strings and JS errors", () => {
    expect(toEngineError({ code: "permissionDenied", message: "no" }).code).toBe(
      "permissionDenied",
    );
    expect(toEngineError("command foo not found")).toMatchObject({
      code: "internal",
      message: "command foo not found",
    });
    expect(toEngineError(new Error("ipc down")).code).toBe("internal");
  });
});
