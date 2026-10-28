import { renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { createMockApi, setApi } from "../../../api";
import { useAppVersion } from "./useAppVersion";

it("gives theme codes the numeric package version, not a dev build's display version", async () => {
  const api = createMockApi();
  setApi(api);
  const info = api.appInfo.bind(api);
  vi.spyOn(api, "appInfo").mockImplementation(async () => ({
    ...(await info()),
    version: "dev~20261028-dirty",
    packageVersion: "0.4.2",
  }));
  const { result } = renderHook(() => useAppVersion());
  await waitFor(() => {
    expect(result.current).toBe("0.4.2");
  });
});
