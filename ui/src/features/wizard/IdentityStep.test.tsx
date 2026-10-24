import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { Identity } from "../../api";
import { setApi } from "../../api";
import { IdentityStep } from "./IdentityStep";
import { pickIdentityFile } from "./pickIdentityFile";
import { renderInWizard, setupWizardTest, type Harness } from "./testUtils";

vi.mock("./pickIdentityFile", () => ({ pickIdentityFile: vi.fn() }));
const pick = vi.mocked(pickIdentityFile);

let harness: Harness;
let onDone: Mock<(identity: Identity) => void>;

beforeEach(async () => {
  harness = await setupWizardTest();
  onDone = vi.fn<(identity: Identity) => void>();
  pick.mockReset();
});

afterEach(() => {
  setApi(undefined);
});

function renderStep() {
  renderInWizard(harness, <IdentityStep identity={null} focusOnMount={false} onDone={onDone} />);
}

function next() {
  fireEvent.click(screen.getByRole("button", { name: "Next" }));
}

describe("IdentityStep", () => {
  it("preselects 'Create new' and shows the backup hint", () => {
    renderStep();
    expect(screen.getByRole("radio", { name: "Create new" })).toBeChecked();
    expect(screen.getByText(/Back it up later in Settings/)).toBeInTheDocument();
  });

  it("asks for a name and focuses the field when it is empty", async () => {
    renderStep();
    next();
    expect(await screen.findByRole("alert")).toHaveTextContent("Please enter a name.");
    const field = screen.getByLabelText("Name");
    expect(field).toHaveFocus();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription(/Please enter a name\./);
    expect(onDone).not.toHaveBeenCalled();
  });

  it("creates the identity when the form is submitted with Enter", async () => {
    renderStep();
    const field = screen.getByLabelText("Name");
    fireEvent.change(field, { target: { value: "  Tobias " } });
    fireEvent.submit(field.closest("form") as HTMLFormElement);
    await waitFor(() => {
      expect(onDone).toHaveBeenCalledWith({ name: "Tobias", certHash: "selfhash" });
    });
  });

  it("explains a secure storage failure", async () => {
    harness.api.mock.failNext("identityCreate", "secureStorageUnavailable");
    renderStep();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Tobias" } });
    next();
    expect(await screen.findByRole("alert")).toHaveTextContent("secure storage");
    expect(onDone).not.toHaveBeenCalled();
  });

  it("requires a file before importing", async () => {
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    next();
    expect(await screen.findByRole("alert")).toHaveTextContent("choose an identity file");
    expect(screen.getByRole("button", { name: "Choose file…" })).toHaveFocus();
  });

  it("imports a chosen file with its password", async () => {
    pick.mockResolvedValue("/home/me/Tobias.p12");
    const importSpy = vi.spyOn(harness.api, "identityImport");
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    expect(await screen.findByText("Chosen file: Tobias.p12")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("File password (if any)"), {
      target: { value: "secret" },
    });
    next();
    await waitFor(() => {
      expect(onDone).toHaveBeenCalledWith({ name: "Tobias", certHash: "importedhash" });
    });
    expect(importSpy).toHaveBeenCalledWith("/home/me/Tobias.p12", "secret");
  });

  it("passes no password when the field is empty", async () => {
    pick.mockResolvedValue("/home/me/Tobias.p12");
    const importSpy = vi.spyOn(harness.api, "identityImport");
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    await screen.findByText("Chosen file: Tobias.p12");
    next();
    await waitFor(() => {
      expect(importSpy).toHaveBeenCalledWith("/home/me/Tobias.p12", null);
    });
  });

  it.each([
    ["invalidInput", "Check the password"],
    ["notFound", "could not be found"],
    ["io", "could not be read"],
  ] as const)("explains an import failure (%s)", async (code, text) => {
    pick.mockResolvedValue("/home/me/Tobias.p12");
    harness.api.mock.failNext("identityImport", code);
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    await screen.findByText("Chosen file: Tobias.p12");
    next();
    expect(await screen.findByRole("alert")).toHaveTextContent(text);
    expect(onDone).not.toHaveBeenCalled();
  });

  it("keeps the previous choice when the picker is cancelled", async () => {
    pick.mockResolvedValue(null);
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    await waitFor(() => {
      expect(pick).toHaveBeenCalled();
    });
    expect(screen.getByText("No file chosen")).toBeInTheDocument();
  });

  it("reports a broken file picker", async () => {
    pick.mockRejectedValue(new Error("no dialog"));
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    renderStep();
    fireEvent.click(screen.getByRole("radio", { name: "Import file" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose file…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("file picker");
  });
});
