import { useTranslation } from "react-i18next";
import { Section } from "../components/Section";
import { InGameOverlayGroup } from "./InGameOverlayGroup";
import { unavailableInGameApi, type InGameOverlayApi } from "./inGameOverlay";
import { WindowOverlayGroup } from "./WindowOverlayGroup";

/**
 * Overlay settings (FR-1.44 – FR-1.46b): the window overlay and the in-game overlay.
 * Positional audio is a later stage and has no settings yet. `inGameApi` defaults to a
 * placeholder until the engine's in-game overlay commands are wired in.
 */
export function OverlaySection({
  inGameApi = unavailableInGameApi,
}: {
  inGameApi?: InGameOverlayApi;
}) {
  const { t } = useTranslation("settings");
  return (
    <Section title={t("sections.overlay")}>
      <WindowOverlayGroup />
      <InGameOverlayGroup api={inGameApi} />
    </Section>
  );
}
