// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";
import { turnCameraOn } from "../specs/camera/flows.ts";
import { waitForAbsent } from "./dom.ts";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  click: vi.fn(),
  staleLookup: vi.fn(),
  execute: vi.fn(),
  waitUntil: vi.fn(),
}));

vi.mock("./index.ts", () => ({ app: { invoke: mocks.invoke }, apps: {} }));
vi.mock("./files-ui.ts", () => ({ registerOnServer: vi.fn() }));
vi.mock("./settings-ui.ts", () => ({ openAllSettings: vi.fn() }));
vi.mock("./voice-ui.ts", () => ({
  addAndConnect: vi.fn(),
  completeWizard: vi.fn(),
  joinIfReadOnly: vi.fn(),
}));
vi.mock("@wdio/globals", () => ({
  $: () => ({
    waitForClickable: vi.fn(),
    waitForDisplayed: vi.fn(),
    click: vi.fn(),
    waitForExist: mocks.staleLookup,
    $: () => ({ waitForEnabled: vi.fn(), click: mocks.click }),
  }),
  browser: { execute: mocks.execute, waitUntil: mocks.waitUntil },
}));

const onAir = { serverId: 1, state: "onAir", error: null, broadcast: "test", viewers: [] };

beforeEach(() => {
  vi.resetAllMocks();
  document.body.innerHTML = '<dialog class="cam-dialog" open></dialog>';
  mocks.invoke.mockResolvedValue(onAir);
  // WebKit can lose the node while a WebDriver element query races dialog unmounting.
  mocks.staleLookup.mockRejectedValue(new Error("stale element reference"));
  mocks.waitUntil.mockImplementation(
    async (condition: () => Promise<boolean>, options?: { timeoutMsg?: string }) => {
      for (let attempt = 0; attempt < 3; attempt++) {
        if (await condition()) return;
      }
      throw new Error(options?.timeoutMsg ?? "condition did not become true");
    },
  );
});

it("waits for preview removal without creating a WebDriver handle to the disappearing node", async () => {
  mocks.execute.mockImplementation((condition: (query: string) => boolean, query: string) => {
    const result = condition(query);
    document.querySelector("dialog.cam-dialog")?.remove();
    return Promise.resolve(result);
  });

  await expect(turnCameraOn()).resolves.toEqual(onAir);
  expect(mocks.execute).toHaveBeenCalledTimes(2);
  expect(mocks.staleLookup).not.toHaveBeenCalled();
  expect(mocks.click).toHaveBeenCalledTimes(1);
  expect(mocks.invoke).toHaveBeenCalledWith("camera_view");
  expect(mocks.waitUntil.mock.calls[0]?.[1]).toEqual({
    timeout: 20_000,
    timeoutMsg: "the camera preview did not close",
  });
});

it("still fails if the preview never closes, even if the engine would be on air", async () => {
  mocks.execute.mockImplementation((condition: (query: string) => boolean, query: string) =>
    Promise.resolve(condition(query)),
  );

  await expect(turnCameraOn()).rejects.toThrow("the camera preview did not close");
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(mocks.click).toHaveBeenCalledTimes(1);
});

it("still waits for the engine to be on air after the dialog closes", async () => {
  document.querySelector("dialog.cam-dialog")?.remove();
  mocks.execute.mockImplementation((condition: (query: string) => boolean, query: string) =>
    Promise.resolve(condition(query)),
  );
  mocks.invoke.mockResolvedValueOnce({ ...onAir, state: "requesting" }).mockResolvedValue(onAir);

  await expect(turnCameraOn()).resolves.toEqual(onAir);
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
});

it("waits for an XPath-selected person to leave without matching another person's row", async () => {
  document.body.innerHTML =
    '<div class="tree-person" id="bob"><span class="tree-person__name">Bob</span></div>' +
    '<div class="tree-person" id="carol"><span class="tree-person__name">Carol</span></div>';
  mocks.execute.mockImplementation((condition: (query: string) => boolean, query: string) => {
    const result = condition(query);
    document.getElementById("bob")?.remove();
    return Promise.resolve(result);
  });
  const options = { timeout: 30_000, timeoutMsg: "Bob's person row remained after disconnecting" };
  await waitForAbsent(
    "//div[contains(@class,'tree-person')][.//span[contains(@class,'tree-person__name') and text()='Bob']]",
    options,
  );
  expect(mocks.execute).toHaveBeenCalledTimes(2);
  expect(document.getElementById("carol")).not.toBeNull();
  expect(mocks.waitUntil).toHaveBeenCalledWith(expect.any(Function), options);
});

it("does not mistake an invalid selector for an absent element", async () => {
  mocks.execute.mockImplementation((condition: (query: string) => boolean, query: string) =>
    Promise.resolve(condition(query)),
  );
  await expect(
    waitForAbsent("[", { timeout: 20_000, timeoutMsg: "invalid selector" }),
  ).rejects.toThrow();
});
