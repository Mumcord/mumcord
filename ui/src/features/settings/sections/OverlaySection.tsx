import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../../api";
import { Section } from "../components/Section";
import { GamePluginsGroup } from "./GamePluginsGroup";
import { InGameOverlayGroup } from "./InGameOverlayGroup";
import { createInGameOverlayApi, type InGameOverlayApi } from "./inGameOverlay";
import { WindowOverlayGroup } from "./WindowOverlayGroup";

/**
 * Overlay settings (FR-1.44 – FR-1.46b): the window overlay and the in-game overlay, plus the
 * positional audio sources (FR-1.42, FR-1.43). `inGameApi` defaults to the engine's in-game
 * overlay commands; tests pass their own.
 */
export function OverlaySection({ inGameApi }: { inGameApi?: InGameOverlayApi }) {
  const { t } = useTranslation("settings");
  const engineInGameApi = useMemo(() => createInGameOverlayApi(getApi()), []);
  return (
    <Section title={t("sections.overlay")}>
      <WindowOverlayGroup />
      <InGameOverlayGroup api={inGameApi ?? engineInGameApi} />
      <GamePluginsGroup />
    </Section>
  );
}
