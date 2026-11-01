import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Language } from "../api";
import { applyLanguage } from "../i18n";
import { LANGUAGE_OPTIONS } from "../i18n/languageOptions";
import { LanguagePicker } from "./LanguagePicker";
import { filterOptions } from "./searchableOptions";

const codes = (query: string) => filterOptions(LANGUAGE_OPTIONS, query).map((o) => o.value);

describe("language filter", () => {
  it("shows every language under its own name for an empty search", () => {
    expect(codes("")).toHaveLength(24);
    expect(codes("  ")).toHaveLength(24);
    expect(LANGUAGE_OPTIONS.find((o) => o.value === "el")?.label).toBe("Ελληνικά");
  });

  it("finds a language by its own name, its English name or its code", () => {
    expect(codes("Deutsch")).toEqual(["de"]);
    expect(codes("german")).toEqual(["de"]);
    expect(codes("DE")[0]).toBe("de");
    expect(codes("ελλ")).toEqual(["el"]);
    expect(codes("greek")).toEqual(["el"]);
    // Gaeilge starts with "ga"; Bulgarian and Hungarian only contain it, so they come after.
    expect(codes("ga")).toEqual(["ga", "hu", "bg"]);
  });

  it("ignores case and accents", () => {
    expect(codes("cestina")).toEqual(["cs"]);
    expect(codes("FRANCAIS")).toEqual(["fr"]);
    expect(codes("română")).toEqual(["ro"]);
  });

  it("finds nothing for text no language contains", () => {
    expect(codes("klingon")).toEqual([]);
  });
});

function renderPicker(value: Language | null = null) {
  const onChange = vi.fn<(language: Language | null) => void>();
  render(
    <>
      <label htmlFor="lang">Display language</label>
      <LanguagePicker id="lang" value={value} onChange={onChange} />
    </>,
  );
  return { onChange, field: screen.getByRole("combobox", { name: "Display language" }) };
}

describe("LanguagePicker", () => {
  beforeAll(async () => {
    await applyLanguage("en");
  });

  it("shows the chosen language and opens a listbox with the system option first", () => {
    const { field } = renderPicker("fr");
    expect(field).toHaveValue("Français");
    expect(field).toHaveAttribute("aria-expanded", "false");
    fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(field).toHaveAttribute("aria-expanded", "true");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(25);
    expect(options[0]).toHaveTextContent("Follow system");
    expect(screen.getByRole("option", { name: "Français" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(field).toHaveAttribute(
      "aria-activedescendant",
      screen.getByRole("option", { name: "Français" }).id,
    );
  });

  it("filters while typing and picks the first match with Enter", () => {
    const { field, onChange } = renderPicker();
    fireEvent.change(field, { target: { value: "germ" } });
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Deutsch"]);
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("de");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("moves with the arrow keys and Home, and chooses the system option as null", () => {
    const { field, onChange } = renderPicker("bg");
    fireEvent.keyDown(field, { key: "ArrowDown" });
    fireEvent.keyDown(field, { key: "ArrowUp" });
    expect(field).toHaveAttribute("aria-activedescendant", screen.getAllByRole("option")[23]?.id);
    fireEvent.keyDown(field, { key: "Home" });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("says when nothing matches", () => {
    const { field } = renderPicker();
    fireEvent.change(field, { target: { value: "klingon" } });
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching language");
  });

  it("closes on Escape and restores the chosen name, then lets Escape through", () => {
    const { field, onChange } = renderPicker("sv");
    const outer = vi.fn();
    document.addEventListener("keydown", outer);
    fireEvent.change(field, { target: { value: "ita" } });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(field).toHaveValue("Svenska");
    expect(outer).not.toHaveBeenCalled();
    fireEvent.keyDown(field, { key: "Escape" });
    expect(outer).toHaveBeenCalledTimes(1);
    expect(onChange).not.toHaveBeenCalled();
    document.removeEventListener("keydown", outer);
  });
});
