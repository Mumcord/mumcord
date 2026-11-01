import { act, render, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { setSettingsOpener } from "../settings/settingsOpener";
import { requestPluginScreen } from "./notifications/pluginRequests";
import {
  consumePluginRequest,
  consumeRequestedPluginTab,
  resetPluginRequestStore,
  usePendingPluginRequest,
  usePluginRequestRouter,
  useRequestedPluginTab,
} from "./pluginRequestStore";

function Router() {
  usePluginRequestRouter();
  return null;
}

afterEach(() => {
  setSettingsOpener(null);
  resetPluginRequestStore();
});

describe("plugin request router", () => {
  it("opens Settings → Plugins on the right tab and keeps the request for it", () => {
    const opener = vi.fn();
    setSettingsOpener(opener);
    const router = render(<Router />);
    const tab = renderHook(() => useRequestedPluginTab());
    const installed = renderHook(() => usePendingPluginRequest("installed"));
    const browse = renderHook(() => usePendingPluginRequest("details"));

    act(() => {
      requestPluginScreen({ kind: "installed", plugin: "dice" });
    });
    expect(opener).toHaveBeenCalledWith("plugins");
    expect(tab.result.current).toBe("installed");
    expect(installed.result.current).toEqual({ kind: "installed", plugin: "dice" });
    expect(browse.result.current).toBeNull();

    act(() => {
      consumePluginRequest("installed");
      consumeRequestedPluginTab();
    });
    expect(installed.result.current).toBeNull();
    expect(tab.result.current).toBeNull();
    router.unmount();
  });

  it("maps every request kind to its tab", () => {
    setSettingsOpener(vi.fn());
    const router = render(<Router />);
    const tab = renderHook(() => useRequestedPluginTab());
    const cases = [
      [{ kind: "details", plugin: "poll" }, "browse"],
      [{ kind: "install", plugin: "poll" }, "browse"],
      [{ kind: "updates" }, "updates"],
      [
        { kind: "addRepository", url: "https://example.test/index.json", key: null },
        "repositories",
      ],
    ] as const;
    for (const [request, expected] of cases) {
      act(() => {
        requestPluginScreen(request);
      });
      expect(tab.result.current).toBe(expected);
    }
    router.unmount();
  });

  it("delivers a request sent before the router mounted", () => {
    setSettingsOpener(vi.fn());
    requestPluginScreen({ kind: "updates" });
    const router = render(<Router />);
    const updates = renderHook(() => usePendingPluginRequest("updates"));
    expect(updates.result.current).toEqual({ kind: "updates" });
    router.unmount();
  });
});
