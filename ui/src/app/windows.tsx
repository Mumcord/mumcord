/**
 * The windows besides the main one (packet inspector, licences, "What's new", media pop-outs).
 * The main window never shows them, so each loads as a chunk of its own (issue #323).
 */
import { lazy } from "react";
import { LazyBoundary } from "../components/LazyBoundary";
import type { MediaPopoutKind } from "../api";

const LazyInspectorWindow = lazy(() =>
  import("../features/inspector/InspectorWindow").then((m) => ({ default: m.InspectorWindow })),
);
const LazyLicencesWindow = lazy(() =>
  import("../features/licences/LicencesWindow").then((m) => ({ default: m.LicencesWindow })),
);
const LazyChangelogWindow = lazy(() =>
  import("../features/changelog/ChangelogWindow").then((m) => ({ default: m.ChangelogWindow })),
);
const LazyMediaPopoutWindow = lazy(() =>
  import("../features/media/MediaPopoutWindow").then((m) => ({ default: m.MediaPopoutWindow })),
);

export function InspectorWindow() {
  return (
    <LazyBoundary>
      <LazyInspectorWindow />
    </LazyBoundary>
  );
}

export function LicencesWindow() {
  return (
    <LazyBoundary>
      <LazyLicencesWindow />
    </LazyBoundary>
  );
}

export function ChangelogWindow() {
  return (
    <LazyBoundary>
      <LazyChangelogWindow />
    </LazyBoundary>
  );
}

export function MediaPopoutWindow({ kind }: { kind: MediaPopoutKind }) {
  return (
    <LazyBoundary>
      <LazyMediaPopoutWindow kind={kind} />
    </LazyBoundary>
  );
}
