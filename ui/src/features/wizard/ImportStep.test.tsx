import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createMockApi, setApi } from "../../api";
import { DEMO_MUMBLE, DEMO_MUMBLE_CERT_PASSWORD } from "../../api/mock";
import { axeViolations } from "../../test/axe";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";
import { Wizard } from "./Wizard";

let harness: Harness;

beforeEach(async () => {
  harness = await setupWizardTest();
  harness.api = createMockApi({ seed: "fresh", mumble: DEMO_MUMBLE });
  setApi(harness.api);
});

afterEach(() => {
  setApi(undefined);
});

async function importStep() {
  renderInWizard(harness, <Wizard />);
  fireEvent.click(await screen.findByRole("button", { name: "Let's get started" }));
  return screen.findByRole("heading", { name: "Bring your Mumble setup along" });
}

describe("Wizard import from the official Mumble client (issue #66)", () => {
  it("comes after the welcome when Mumble is installed, with a preview of what it finds", async () => {
    await importStep();
    expect(screen.getByText("Step 2 of 11 · optional")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Server favourites" })).toBeChecked();
    expect(screen.getByText("6 found")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Saved passwords" })).toBeChecked();
    expect(screen.getByText("Stored in the system keychain")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Identity (client certificate)" })).toBeChecked();
    expect(screen.getByText("Password needed")).toBeInTheDocument();
    expect(screen.getByText("Microphone: USB Headset · Output: Speakers")).toBeInTheDocument();
    expect(screen.getByText("Ctrl + Space")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Volume and voice settings" })).not.toBeChecked();
    expect(screen.getByText("5 items selected")).toBeInTheDocument();
    expect(await axeViolations(document.body)).toEqual([]);
  });

  it("skips to the identity step without importing", async () => {
    await importStep();
    fireEvent.click(screen.getByRole("button", { name: "Skip for now" }));
    await screen.findByRole("heading", { name: "Who are you on Mumble?" });
    expect(screen.getByText("Step 3 of 11 · Identity")).toBeInTheDocument();
    expect(await harness.api.serversList()).toEqual([]);
  });

  it("asks for the certificate password, imports and can undo", async () => {
    await importStep();
    // Leave one favourite out through the review list.
    fireEvent.click(screen.getByRole("button", { name: "Review list" }));
    const list = screen.getByRole("list", { name: "Servers to copy" });
    fireEvent.click(within(list).getByRole("checkbox", { name: /Silent Orbit/ }));

    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Please enter the certificate password.",
    );
    const password = screen.getByLabelText("Certificate password from Mumble");
    await waitFor(() => expect(password).toHaveFocus());
    fireEvent.change(password, { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("does not open the certificate");
    expect(await harness.api.serversList()).toEqual([]);

    fireEvent.change(password, { target: { value: DEMO_MUMBLE_CERT_PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    const done = await screen.findByRole("heading", { name: "Import finished" });
    await waitFor(() => expect(done).toHaveFocus());
    expect(screen.getByText("5 servers added to favourites")).toBeInTheDocument();
    expect(screen.getByText("3 passwords saved")).toBeInTheDocument();
    expect(screen.getByText("Identity imported")).toBeInTheDocument();
    expect(screen.getByText("Audio devices and push-to-talk key")).toBeInTheDocument();
    const settings = await harness.api.settingsGet();
    expect(settings).toMatchObject({
      inputDevice: "pipewire:usb-headset",
      pttKey: "Ctrl+Space",
      talkMode: "pushToTalk",
    });
    expect(await harness.api.identityGet()).toMatchObject({ name: "Zoe" });

    fireEvent.click(screen.getByRole("button", { name: "Undo import" }));
    await screen.findByRole("heading", { name: "Bring your Mumble setup along" });
    expect(await harness.api.serversList()).toEqual([]);
    expect(await harness.api.identityGet()).toBeNull();
    expect(await harness.api.settingsGet()).toMatchObject({ pttKey: null, inputDevice: null });
  });

  it("continues to the identity step after Done when no certificate was chosen", async () => {
    await importStep();
    fireEvent.click(screen.getByRole("checkbox", { name: "Identity (client certificate)" }));
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    await screen.findByRole("heading", { name: "Import finished" });
    expect(screen.queryByText("Identity imported")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await screen.findByRole("heading", { name: "Who are you on Mumble?" });
    expect(screen.getByText("Step 3 of 11 · Identity")).toBeInTheDocument();
  });

  it("uses the imported certificate as the identity", async () => {
    await importStep();
    fireEvent.change(screen.getByLabelText("Certificate password from Mumble"), {
      target: { value: DEMO_MUMBLE_CERT_PASSWORD },
    });
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    await screen.findByRole("heading", { name: "Import finished" });
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await screen.findByRole("heading", { name: "Who are you on Mumble?" });
    expect(screen.getByText("Using identity Zoe.")).toBeInTheDocument();
  });
});
