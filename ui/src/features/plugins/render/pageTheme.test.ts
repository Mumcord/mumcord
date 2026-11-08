import { afterEach, describe, expect, it, vi } from "vitest";
import { onPageThemeChange, PAGE_TOKEN_VARS, readPageTheme } from "./pageTheme";

/** The token set of FR-13.9 (API 1.2). Plugin pages rely on every one of them. */
const FR_13_9 = {
  "--mc-color-text": "--color-text",
  "--mc-color-text-muted": "--color-text-muted",
  "--mc-color-accent": "--color-accent",
  "--mc-color-danger": "--color-danger",
  "--mc-color-warning": "--color-warning",
  "--mc-color-success": "--color-success",
  "--mc-color-surface": "--color-bg-chat",
  "--mc-color-surface-raised": "--color-bg-elevated",
  "--mc-color-border": "--color-border",
  "--mc-color-text-secondary": "--color-text-secondary",
  "--mc-color-accent-text": "--color-accent-text",
  "--mc-color-on-accent": "--color-on-accent",
  "--mc-color-primary": "--color-primary-bg",
  "--mc-color-on-primary": "--color-primary-text",
  "--mc-color-hover": "--color-bg-hover",
  "--mc-color-selected": "--color-bg-selected",
  "--mc-color-border-strong": "--color-border-strong",
  "--mc-color-focus": "--color-focus",
  "--mc-color-backdrop": "--color-backdrop",
  "--mc-font-family": "--font-family",
  "--mc-font-family-display": "--font-family-display",
  "--mc-font-family-mono": "--font-family-mono",
  "--mc-font-size-xs": "--font-size-xs",
  "--mc-font-size-sm": "--font-size-sm",
  "--mc-font-size-md": "--font-size-md",
  "--mc-font-size-lg": "--font-size-lg",
  "--mc-font-size-xl": "--font-size-xl",
  "--mc-font-weight-normal": "--font-weight-normal",
  "--mc-font-weight-bold": "--font-weight-bold",
  "--mc-line-height": "--line-height",
  "--mc-space-1": "--space-1",
  "--mc-space-2": "--space-2",
  "--mc-space-3": "--space-3",
  "--mc-space-4": "--space-4",
  "--mc-space-5": "--space-5",
  "--mc-space-6": "--space-6",
  "--mc-radius-sm": "--radius-sm",
  "--mc-radius-md": "--radius-md",
  "--mc-radius-lg": "--radius-lg",
  "--mc-radius-full": "--radius-full",
  "--mc-focus-ring-width": "--focus-ring-width",
  "--mc-shadow-raised": "--shadow-raised",
  "--mc-shadow-primary-glow": "--shadow-primary-glow",
  "--mc-duration-fast": "--duration-fast",
  "--mc-duration-normal": "--duration-normal",
  "--mc-ease-out": "--ease-out",
};

const root = document.documentElement;

function setAppVars(values: Record<string, string>) {
  for (const [name, value] of Object.entries(values)) root.style.setProperty(name, value);
}

function mockMedia(matching: string[]) {
  vi.stubGlobal(
    "matchMedia",
    (query: string) =>
      ({
        matches: matching.includes(query),
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
      }) as unknown as MediaQueryList,
  );
}

async function flush() {
  await Promise.resolve();
}

afterEach(() => {
  vi.unstubAllGlobals();
  root.removeAttribute("style");
  root.dataset.theme = "dark";
  delete root.dataset.effects;
});

describe("page theme tokens", () => {
  it("are exactly the tokens of FR-13.9", () => {
    expect(PAGE_TOKEN_VARS).toEqual(FR_13_9);
  });

  it("carry the app's current values and the mode attributes", () => {
    root.dataset.theme = "light";
    setAppVars({ "--color-text": "#123456", "--duration-fast": "120ms", "--space-1": "0.25rem" });
    const theme = readPageTheme();
    expect(theme.mode).toBe("light");
    expect(theme.scheme).toBe("light");
    expect(theme.reducedMotion).toBe(false);
    expect(theme.tokens["--mc-color-text"]).toBe("#123456");
    expect(theme.tokens["--mc-duration-fast"]).toBe("120ms");
    expect(theme.tokens["--mc-space-1"]).toBe("0.25rem");
  });

  it("carry the primary glow as the current theme resolves it", () => {
    const glow = "0 0.5rem 1.5rem -0.25rem color-mix(in srgb, #7c5cff 55%, transparent)";
    setAppVars({ "--shadow-primary-glow": glow });
    expect(readPageTheme().tokens["--mc-shadow-primary-glow"]).toBe(glow);
    // A custom theme's accent gives a different glow, and the page gets that one.
    const custom = "0 0.5rem 1.5rem -0.25rem color-mix(in srgb, #00aa55 55%, transparent)";
    setAppVars({ "--shadow-primary-glow": custom.replace(" color-mix", "\n    color-mix") });
    expect(readPageTheme().tokens["--mc-shadow-primary-glow"]).toBe(custom);
  });

  it("are high contrast while the system forces colours, keeping the base scheme", () => {
    mockMedia(["(forced-colors: active)"]);
    root.dataset.theme = "dark";
    const theme = readPageTheme();
    expect(theme.mode).toBe("high-contrast");
    expect(theme.scheme).toBe("dark");
  });

  it("zero the durations under reduced motion or with visual effects off", () => {
    setAppVars({ "--duration-fast": "120ms", "--duration-normal": "200ms" });
    root.dataset.effects = "off";
    const theme = readPageTheme();
    expect(theme.reducedMotion).toBe(true);
    expect(theme.tokens["--mc-duration-fast"]).toBe("0ms");
    expect(theme.tokens["--mc-duration-normal"]).toBe("0ms");

    delete root.dataset.effects;
    mockMedia(["(prefers-reduced-motion: reduce)"]);
    expect(readPageTheme().tokens["--mc-duration-fast"]).toBe("0ms");
  });
});

describe("onPageThemeChange", () => {
  it("reports changes of the shown theme, not the theme at subscribing", async () => {
    const listener = vi.fn();
    const stop = onPageThemeChange(listener);
    await flush();
    expect(listener).not.toHaveBeenCalled();

    // A custom theme edit sets inline colours on <html>.
    setAppVars({ "--color-accent": "#ff0000" });
    await flush();
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener.mock.lastCall?.[0]).toMatchObject({
      tokens: { "--mc-color-accent": "#ff0000" },
    });

    root.dataset.theme = "light";
    await flush();
    expect(listener).toHaveBeenCalledTimes(2);
    expect(listener.mock.lastCall?.[0]).toMatchObject({ mode: "light", scheme: "light" });

    // Unrelated attributes change nothing.
    root.setAttribute("lang", "de");
    await flush();
    expect(listener).toHaveBeenCalledTimes(2);

    stop();
    root.dataset.theme = "dark";
    await flush();
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
