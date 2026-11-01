/**
 * The mock has no peer, so it cannot send frames. In the mock UI the media views show the mock's
 * placeholder picture instead, which is what makes them visible in a screenshot.
 */
import { getApi } from "../../api";
import type { MockApi } from "../../api/mock";

/** The mock's placeholder picture, or `null` when the UI runs against the engine. */
export function mockPlaceholder(): string | null {
  const api = getApi();
  if (api.kind !== "mock") return null;
  const picture = (api as MockApi).mediaDemo.picture;
  return picture === "" ? null : picture;
}
