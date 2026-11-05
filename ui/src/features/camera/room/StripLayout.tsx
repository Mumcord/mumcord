/**
 * The combined layout of the media area (spec 08 §5.4, FR-8.71–8.74): while a watched screen
 * share or the joined watch party is the main item and the camera room is open in the main
 * window, that picture stays the main one and the cameras become a strip beside it, above it or
 * hidden, inside one dark card with one header, made of the strip header and the main item's own
 * line and chips, and the room bar.
 *
 * The media area's items are always rendered at the same place in the tree, so a share or a party
 * never restarts when the strip comes or goes (FR-8.74); without the strip the wrapper takes no
 * room at all. The strip itself loads with the camera room's code.
 */
import { lazy, Suspense, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useVoiceServerId } from "../../channels";
import { CombinedHeaderContext, type CombinedHeaderSlots } from "../../media/combinedHeader";
import { useStageEntries } from "../../media/stageRegistry";
import { useStripMain, useStripState } from "./StripState";

const StripRoom = lazy(() => import("./StripRoom").then((m) => ({ default: m.StripRoom })));

export function StripLayout({ children }: { children: ReactNode }) {
  const serverId = useVoiceServerId();
  const main = useStripMain(serverId);
  const { position, large } = useStripState();
  const { t } = useTranslation("camera");
  const { t: tc } = useTranslation("common");
  const { labels } = useStageEntries();
  const ref = useRef<HTMLElement>(null);
  const [sub, setSub] = useState<HTMLElement | null>(null);
  const [chips, setChips] = useState<HTMLElement | null>(null);
  const combined = serverId !== null && main !== null;
  // The view draws its line and chips into the one header while there is one (mockups 08–09).
  const slots = useMemo<CombinedHeaderSlots | null>(
    () => (combined && sub !== null && chips !== null ? { sub, chips } : null),
    [combined, sub, chips],
  );
  const title =
    main === "share"
      ? (labels.share ?? tc("media.stage.share"))
      : (labels.watch ?? t("combined.partyTitle"));
  return (
    <section
      ref={ref}
      className={combined ? "cam-room cam-combined" : "cam-combined-off"}
      aria-label={combined ? t("combined.label", { title }) : undefined}
      data-position={combined ? position : undefined}
      data-large={(combined && large !== null) || undefined}
    >
      <div className="cam-combined__main">
        <CombinedHeaderContext value={slots}>{children}</CombinedHeaderContext>
      </div>
      {combined && (
        <Suspense fallback={null}>
          <StripRoom
            serverId={serverId}
            main={main}
            title={title}
            frame={ref}
            subSlot={setSub}
            chipsSlot={setChips}
          />
        </Suspense>
      )}
    </section>
  );
}
