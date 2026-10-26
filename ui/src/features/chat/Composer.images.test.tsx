import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockApi } from "../../api";
import { connectedMock, resetAll } from "../channels/testUtils";
import { Composer } from "./Composer";

const prepare = vi.hoisted(() => vi.fn());
vi.mock("./images", async (original) => ({
  ...(await original<typeof import("./images")>()),
  prepareImage: prepare,
}));

const LIMITS = { text: 5000, image: 100_000, allowHtml: true };

function png(name: string, type = "image/png"): File {
  return new File(["x"], name, { type });
}

function ready(tag: string, scaled = false) {
  return { ok: true, dataUrl: `data:image/png;base64,${tag}`, width: 800, height: 600, scaled };
}

let api: MockApi;
let sent: string[];

beforeEach(async () => {
  prepare.mockReset();
  prepare.mockImplementation((file: File) => Promise.resolve(ready(file.name)));
  api = await connectedMock();
  sent = [];
  const original = api.sendChannelMessage.bind(api);
  vi.spyOn(api, "sendChannelMessage").mockImplementation((serverId, channel, html) => {
    sent.push(html);
    return original(serverId, channel, html);
  });
});

afterEach(() => {
  resetAll();
});

function setup() {
  const { container } = render(
    <Composer
      target={{ kind: "channel", channelId: 1 }}
      limits={LIMITS}
      label="Message #Lobby"
      blockedReason={null}
    />,
  );
  const input = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Message #Lobby" });
  const form = container.querySelector("form") as HTMLFormElement;
  return { input, form };
}

function paste(input: HTMLElement, files: File[]) {
  fireEvent.paste(input, { clipboardData: { files, types: ["Files"] } });
}

function drop(form: HTMLElement, files: File[]) {
  fireEvent.drop(form, { dataTransfer: { files, types: ["Files"] } });
}

describe("Composer image attachments", () => {
  it("shows a pasted image as a thumbnail with a remove button", async () => {
    const { input } = setup();
    paste(input, [png("a.png")]);
    expect(await screen.findByRole("img", { name: "Image to send: a.png" })).toBeTruthy();
    expect(screen.getByText("800×600, 1 KB")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Remove image a.png" }));
    expect(screen.queryByRole("img", { name: /Image to send/ })).toBeNull();
  });

  it("takes three dropped images and sends the text, then each image in order", async () => {
    const { input, form } = setup();
    drop(form, [png("a.png"), png("b.png"), png("c.png")]);
    await waitFor(() => {
      expect(screen.getAllByRole("img")).toHaveLength(3);
    });
    fireEvent.change(input, { target: { value: "hello" } });
    fireEvent.submit(form);
    await waitFor(() => {
      expect(sent).toHaveLength(4);
    });
    expect(sent[0]).toContain("hello");
    // Each thumbnail differs from its file, so it carries the kept original's reference (FR-1.60c).
    const full =
      /^<img src="data:image\/png;base64,(\w\.png)" data-mumcord-full="[0-9a-f]{64}:\d+" \/>$/;
    expect(sent.slice(1).map((html) => full.exec(html)?.[1])).toEqual(["a.png", "b.png", "c.png"]);
    await waitFor(() => {
      expect(screen.queryAllByRole("img")).toHaveLength(0);
    });
  });

  it("sends an image on its own without text", async () => {
    const { input, form } = setup();
    paste(input, [png("a.png")]);
    await screen.findByRole("img");
    fireEvent.submit(form);
    await waitFor(() => {
      expect(sent).toHaveLength(1);
    });
  });

  it("shows a spinner and does not send while an image is still processing", async () => {
    let finish: (value: unknown) => void = () => {};
    prepare.mockImplementation(() => new Promise((resolve) => (finish = resolve)));
    const { input, form } = setup();
    paste(input, [png("slow.png")]);
    expect(await screen.findByRole("status", { name: "Preparing image" })).toBeTruthy();
    fireEvent.submit(form);
    expect(sent).toHaveLength(0);
    await act(async () => {
      finish(ready("slow"));
      await Promise.resolve();
    });
    expect(await screen.findByRole("img", { name: /slow/ })).toBeTruthy();
  });

  it("returns from the paste handler before any image work is done", () => {
    prepare.mockImplementation(() => new Promise(() => {}));
    const { input } = setup();
    paste(input, [png("a.png")]);
    expect(screen.getByRole("status", { name: "Preparing image" })).toBeTruthy();
  });

  it("notes when an image was scaled down", async () => {
    prepare.mockResolvedValue(ready("big", true));
    const { input } = setup();
    paste(input, [png("big.png")]);
    expect(await screen.findByText("Scaled to 800×600 to fit the server limit")).toBeTruthy();
  });

  it("refuses non-image files and images that are too large", async () => {
    const { form } = setup();
    drop(form, [png("doc.pdf", "application/pdf")]);
    expect((await screen.findByRole("alert")).textContent).toBe("Only images can be sent.");
    expect(prepare).not.toHaveBeenCalled();

    prepare.mockResolvedValue({ ok: false, reason: "sourceTooLarge" });
    drop(form, [png("huge.png")]);
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain("over 25 MB");
    });
    expect(screen.queryAllByRole("img")).toHaveLength(0);
  });

  it("removes the last image with Backspace in an empty input only", async () => {
    const { input } = setup();
    paste(input, [png("a.png"), png("b.png")]);
    await waitFor(() => {
      expect(screen.getAllByRole("img")).toHaveLength(2);
    });
    fireEvent.change(input, { target: { value: "x" } });
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(screen.getAllByRole("img")).toHaveLength(2);
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.keyDown(input, { key: "Backspace" });
    expect(screen.getAllByRole("img")).toHaveLength(1);
    expect(screen.getByRole("img", { name: /a\.png/ })).toBeTruthy();
  });

  it("highlights the composer while files are dragged over it", () => {
    const { form } = setup();
    fireEvent.dragEnter(form, { dataTransfer: { types: ["Files"] } });
    expect(form.dataset.dragging).toBe("true");
    fireEvent.dragLeave(form, { dataTransfer: { types: ["Files"] } });
    expect(form.dataset.dragging).toBeUndefined();
  });

  it("never inserts a dropped file:// link as text", () => {
    const { input } = setup();
    // WebKitGTK delivers a file from the file manager only as a link, without File objects.
    const notCancelled = fireEvent.drop(input, {
      dataTransfer: { files: [], types: ["text/uri-list", "text/plain"] },
    });
    expect(notCancelled).toBe(false);
    expect(input.value).toBe("");
  });
});
