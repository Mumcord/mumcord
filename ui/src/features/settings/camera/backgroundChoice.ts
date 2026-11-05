import type { CameraBackground } from "../../../api";

/** Value of the "Default background" select: None, Blur, the built-in library or one own image. */
export type BackgroundChoice = "none" | "blur" | "library" | `own:${string}`;

/** The select value that shows `background`. An own image that no longer exists shows Blur. */
export function choiceOf(
  background: CameraBackground,
  ownIds: readonly string[],
): BackgroundChoice {
  switch (background.kind) {
    case "none":
      return "none";
    case "blur":
      return "blur";
    case "builtin":
      return "library";
    case "own":
      return ownIds.includes(background.id) ? `own:${background.id}` : "blur";
  }
}

/**
 * The background to save for `choice`. "Image: Library" keeps the built-in image already chosen
 * (e.g. in the preview dialog) and otherwise takes the first one of the library.
 */
export function backgroundOf(
  choice: BackgroundChoice,
  current: CameraBackground,
  builtinIds: readonly string[],
): CameraBackground | null {
  if (choice === "none" || choice === "blur") return { kind: choice };
  if (choice === "library") {
    if (current.kind === "builtin") return current;
    const first = builtinIds[0];
    return first === undefined ? null : { kind: "builtin", id: first };
  }
  return { kind: "own", id: choice.slice("own:".length) };
}
