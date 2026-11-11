/** Line icons of the rich text editor's toolbar (issue #666), drawn on ComposerIcon's grid. */
export const EDITOR_ICONS = {
  bold: "M7 5h6a3.5 3.5 0 0 1 0 7H7zM7 12h7a3.5 3.5 0 0 1 0 7H7z",
  italic: "M10 5h8M6 19h8M14 5l-4 14",
  underline: "M7 4v7a5 5 0 0 0 10 0V4M5 20h14",
  strike:
    "M5 12h14M16 7.5C15.4 6 14 5 12 5c-2.5 0-4 1.3-4 3M8 16.5c.6 1.5 2 2.5 4 2.5 2.5 0 4-1.3 4-3",
  color: "M6.5 16L12 4l5.5 12M8.6 11.5h6.8",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  image: ["M4 5h16v14H4z", "M4 16l5-5 4 4 2.5-2.5L20 17", "M15.5 8.5h.01"],
  bullets: "M10 6h10M10 12h10M10 18h10M4.5 6h.5M4.5 12h.5M4.5 18h.5",
  numbers: "M10 6h10M10 12h10M10 18h10M4 4.5l1.5-1v5M3.8 14.2c.3-.8 2.7-.9 2.4.6L4 18h2.5",
  source: "M9 8l-4 4 4 4M15 8l4 4-4 4",
  chevron: "M8 10l4 4 4-4",
  warn: "M12 4l9 16H3zM12 10v4M12 17h.01",
} as const;

/** The text colours on offer; the palette adds "default" (no colour) in front. */
export const SWATCHES = [
  { name: "red", value: "#e5484d" },
  { name: "orange", value: "#f76b15" },
  { name: "yellow", value: "#ffc53d" },
  { name: "green", value: "#30a46c" },
  { name: "teal", value: "#12a594" },
  { name: "blue", value: "#3e83f8" },
  { name: "purple", value: "#8e4ec6" },
  { name: "pink", value: "#d6409f" },
  { name: "grey", value: "#8b8d98" },
  { name: "white", value: "#ffffff" },
] as const;
