import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { Modal } from "./Modal";

/** A form whose Save button is replaced by other content, like a plugin's setup turning into its player. */
function SetupThenPlayer({ onClose }: { onClose: () => void }) {
  const [saved, setSaved] = useState(false);
  return (
    <Modal title="Configure bot" onClose={onClose} closeLabel="Close">
      {saved ? (
        <p>Nothing is playing</p>
      ) : (
        <button
          type="button"
          onClick={() => {
            setSaved(true);
          }}
        >
          Save
        </button>
      )}
    </Modal>
  );
}

describe("Modal", () => {
  it("keeps the focus in the dialog when the focused control goes away, so Escape still closes it", async () => {
    const onClose = vi.fn();
    render(<SetupThenPlayer onClose={onClose} />);
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toHaveFocus();
    fireEvent.click(save);
    expect(screen.getByText("Nothing is playing")).toBeInTheDocument();
    const dialog = screen.getByRole("dialog", { name: "Configure bot" });
    await waitFor(() => {
      expect(dialog).toHaveFocus();
    });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
