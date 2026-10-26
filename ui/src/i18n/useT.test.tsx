import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { i18n } from "./index";
import { useT } from "./useT";

function Label() {
  const t = useT("channels");
  return <span>{t("statusTip.positional")}</span>;
}

describe("useT", () => {
  it("translates like useTranslation", () => {
    render(<Label />);
    expect(screen.getByText(i18n.getFixedT("en", "channels")("statusTip.positional"))).toBeTruthy();
  });

  it("does not copy the i18n instance per mount, as useTranslation does", () => {
    const copies = vi.spyOn(Object, "getOwnPropertyDescriptors");
    render(<Label />);
    const forOne = copies.mock.calls.length;
    render(
      <>
        {Array.from({ length: 50 }, (_, i) => (
          <Label key={i} />
        ))}
      </>,
    );
    expect(copies.mock.calls.length).toBe(forOne);
    copies.mockRestore();
  });

  it("re-renders when the language changes", async () => {
    render(<Label />);
    i18n.addResourceBundle("xx", "channels", { statusTip: { positional: "XX-3D" } }, true, true);
    await act(async () => {
      await i18n.changeLanguage("xx");
    });
    expect(screen.getByText("XX-3D")).toBeTruthy();
    await act(async () => {
      await i18n.changeLanguage("en");
    });
  });
});
