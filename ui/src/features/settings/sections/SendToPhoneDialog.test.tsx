import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi, setApi, type Identity, type MumcordApi } from "../../../api";
import { applyLanguage } from "../../../i18n";
import { SendToPhoneDialog } from "./SendToPhoneDialog";
import { DWELL_MS, formatLeft, groupCode, modulePx, qrPath, spokenCode } from "./sendToPhone";

let api: MumcordApi;
let identities: Identity[];

beforeEach(async () => {
  await applyLanguage("en");
  api = createMockApi();
  setApi(api);
  const list = await api.identityList();
  for (const identity of list.slice(1)) await api.identityDelete(identity.id);
  const first = list[0] ?? (await api.identityCreate("Lea", "Lea"));
  await api.identityRename(first.id, "Lea");
  await api.identityCreate("Work", "Work");
  identities = await api.identityList();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  setApi(undefined);
});

function open(serverCount = 4) {
  const onClose = vi.fn();
  const view = render(
    <SendToPhoneDialog identities={identities} serverCount={serverCount} onClose={onClose} />,
  );
  return { onClose, ...view };
}

/** Lets the pending answer of the backend arrive. */
async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

async function tick(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function showCodes() {
  fireEvent.click(screen.getByRole("button", { name: "Show codes" }));
  await settle();
}

function shownCode(): string {
  return screen.getByTestId("phone-transfer-code").textContent;
}

describe("helpers", () => {
  it("groups the code in two halves and spells it out for screen readers", () => {
    expect(groupCode("41820735")).toBe("4182 0735");
    expect(spokenCode("41820735")).toBe("4 1 8 2 0 7 3 5");
  });

  it("formats the remaining time", () => {
    expect(formatLeft(102)).toBe("1:42");
    expect(formatLeft(120)).toBe("2:00");
    expect(formatLeft(9)).toBe("0:09");
    expect(formatLeft(-3)).toBe("0:00");
  });

  it("draws whole pixels per module, four for the usual code of 77 modules", () => {
    expect(modulePx(77)).toBe(4);
    expect(modulePx(21)).toBe(8);
    expect(modulePx(177)).toBe(4);
  });

  it("turns rows into runs inside the quiet zone", () => {
    expect(qrPath({ size: 3, rows: ["110", "000", "011"] })).toBe("M4 4h2v1h-2zM5 6h2v1h-2z");
  });
});

describe("SendToPhoneDialog", () => {
  it("starts with the default identity and the server list chosen and shows no code yet", () => {
    open();
    expect(screen.getByRole("dialog", { name: "Send to your phone" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Identity: Lea (default)" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Identity: Work" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Server list (4 servers)" })).toBeChecked();
    expect(screen.queryByTestId("phone-transfer-qr")).not.toBeInTheDocument();
    expect(screen.queryByTestId("phone-transfer-code")).not.toBeInTheDocument();
    expect(screen.getByText(/Only show them to your own phone/)).toBeInTheDocument();
  });

  it("cannot start with nothing chosen, but can with the server list alone", () => {
    open();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity: Lea (default)" }));
    expect(screen.getByRole("button", { name: "Show codes" })).toBeEnabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Server list (4 servers)" }));
    expect(screen.getByRole("button", { name: "Show codes" })).toBeDisabled();
    expect(screen.getByText("Select at least one entry.")).toBeInTheDocument();
  });

  it("does not offer an empty server list by default", () => {
    open(0);
    expect(screen.getByRole("checkbox", { name: "Server list (0 servers)" })).not.toBeChecked();
  });

  it("shows the codes for the selection, the typed code and the time", async () => {
    const start = vi.spyOn(api, "phoneTransferStart");
    open();
    await showCodes();
    expect(start).toHaveBeenCalledWith([identities[0]?.id], true);
    expect(screen.getByRole("img", { name: "QR code 1 of 3" })).toBeInTheDocument();
    expect(screen.getByText("Code 1 of 3")).toBeInTheDocument();
    expect(shownCode()).toContain("4182 0735");
    expect(shownCode()).toContain("4 1 8 2 0 7 3 5");
    expect(screen.getByText("Expires in 2:00")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show codes" })).not.toBeInTheDocument();
  });

  it("moves focus to the codes, so Enter does not close the dialog", async () => {
    const { onClose } = open();
    await showCodes();
    const codes = screen.getByRole("group", { name: "Codes for your phone" });
    expect(codes).toHaveFocus();
    fireEvent.keyDown(codes, { key: "Enter" });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(codes, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("cycles through the codes and starts over after the last", async () => {
    open();
    await showCodes();
    await tick(DWELL_MS);
    expect(screen.getByRole("img", { name: "QR code 2 of 3" })).toBeInTheDocument();
    await tick(DWELL_MS);
    expect(screen.getByText("Code 3 of 3")).toBeInTheDocument();
    await tick(DWELL_MS);
    expect(screen.getByText("Code 1 of 3")).toBeInTheDocument();
  });

  it("counts down", async () => {
    open();
    await showCodes();
    await tick(18_000);
    expect(screen.getByText("Expires in 1:42")).toBeInTheDocument();
  });

  it("replaces the codes when the time is up and says so", async () => {
    const start = vi.spyOn(api, "phoneTransferStart");
    open();
    await showCodes();
    expect(screen.queryByText(/Start again on your phone/)).not.toBeInTheDocument();
    const before = shownCode();
    await tick(120_000);
    await settle();
    expect(start).toHaveBeenCalledTimes(2);
    expect(shownCode()).not.toBe(before);
    expect(screen.getByText("Expires in 2:00")).toBeInTheDocument();
    expect(
      screen.getByText("New codes and a new number. Start again on your phone."),
    ).toBeInTheDocument();
  });

  it("replaces the codes when the selection changes", async () => {
    const start = vi.spyOn(api, "phoneTransferStart");
    open();
    await showCodes();
    const before = shownCode();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity: Work" }));
    expect(screen.queryByTestId("phone-transfer-code")).not.toBeInTheDocument();
    await settle();
    expect(start).toHaveBeenLastCalledWith([identities[0]?.id, identities[1]?.id], true);
    expect(shownCode()).not.toBe(before);
    expect(screen.getByText(/Start again on your phone/)).toBeInTheDocument();
  });

  it("takes the codes off when nothing is chosen any more", async () => {
    const cancel = vi.spyOn(api, "phoneTransferCancel");
    open();
    await showCodes();
    fireEvent.click(screen.getByRole("checkbox", { name: "Server list (4 servers)" }));
    await settle();
    cancel.mockClear();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity: Lea (default)" }));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("phone-transfer-qr")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show codes" })).toBeDisabled();
  });

  it("ignores the answer to a start that was overtaken", async () => {
    let first: ((value: Awaited<ReturnType<MumcordApi["phoneTransferStart"]>>) => void) | undefined;
    const real = api.phoneTransferStart.bind(api);
    const start = vi.spyOn(api, "phoneTransferStart");
    start.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          first = resolve;
        }),
    );
    open();
    await showCodes();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity: Work" }));
    await settle();
    const current = shownCode();
    const late = await real([], true);
    await act(async () => {
      first?.(late);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(shownCode()).toBe(current);
  });

  it("explains a selection that is too large", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(api, "phoneTransferStart").mockRejectedValueOnce({
      code: "invalidInput",
      message: "the selection is too large for a transfer",
    });
    open();
    await showCodes();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "This is too much for one transfer. Select fewer entries.",
    );
    expect(screen.queryByTestId("phone-transfer-qr")).not.toBeInTheDocument();
  });

  it("drops the transfer and everything shown when it closes", async () => {
    const cancel = vi.spyOn(api, "phoneTransferCancel");
    const start = vi.spyOn(api, "phoneTransferStart");
    const { unmount } = open();
    await showCodes();
    cancel.mockClear();
    unmount();
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("phone-transfer-code")).not.toBeInTheDocument();
    await tick(200_000);
    expect(start).toHaveBeenCalledTimes(1);
  });

  it("closes from Cancel and from Done", async () => {
    const { onClose } = open();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await showCodes();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
