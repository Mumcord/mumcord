import { describe, expect, it } from "vitest";
import { EngineError } from "../../api";
import { watchFailure } from "./watchFailure";

describe("watch failure reasons", () => {
  it.each([
    ["shareFull", "this share is full", "full"],
    ["directUnavailable", "noDirectPath", "unreachable"],
    ["directUnavailable", "peerUnsupported", "tooOld"],
    ["directUnavailable", "consentOff", "consentOff"],
    ["notFound", "the sharer did not answer", "unreachable"],
    ["notConnected", "not connected", "failed"],
    ["internal", "boom", "failed"],
  ] as const)("maps %s (%s) to %s", (code, message, reason) => {
    expect(watchFailure(new EngineError(code, message))).toBe(reason);
  });
});
