import { afterEach, describe, expect, it, vi } from "vitest";
import { decodeBase64 } from "./base64";

const Bytes = Uint8Array;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("decodeBase64", () => {
  it("uses the native decoder when available", () => {
    const native = vi.fn(() => new Bytes([0, 128, 255]));
    vi.stubGlobal(
      "Uint8Array",
      class extends Bytes {
        static fromBase64 = native;
      },
    );
    expect(decodeBase64("AID/")).toEqual(new Bytes([0, 128, 255]));
    expect(native).toHaveBeenCalledExactlyOnceWith("AID/");
  });

  it.each([
    ["", []],
    ["AID/", [0, 128, 255]],
    ["YQ==", [97]],
    ["YQ", [97]],
    ["Y W J j\n", [97, 98, 99]],
  ])("keeps the fallback compatible with atob: %s", (source, expected) => {
    vi.stubGlobal(
      "Uint8Array",
      class extends Bytes {
        static fromBase64 = undefined;
      },
    );
    expect([...decodeBase64(source)]).toEqual(expected);
  });

  it("lets callers handle invalid input on either path", () => {
    vi.stubGlobal(
      "Uint8Array",
      class extends Bytes {
        static fromBase64 = undefined;
      },
    );
    expect(() => decodeBase64("not!base64")).toThrow();
    vi.stubGlobal(
      "Uint8Array",
      class extends Bytes {
        static fromBase64 = () => {
          throw new SyntaxError("invalid base64");
        };
      },
    );
    expect(() => decodeBase64("not!base64")).toThrow(SyntaxError);
  });
});
