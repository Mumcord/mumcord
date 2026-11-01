/**
 * Root of a media pop-out window (issue #164): the watch party player or a watched screen share
 * with the same overlay and keys as in the main window. The window only renders: the party, the
 * watch and the connection stay in the main window, this one reads their state from the engine and
 * sends its button presses back. Theme and language follow the main window; Ctrl+W, closing the
 * window and "Pop back in" dock the view again.
 */
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { LiveRegions } from "../../a11y";
import { getApi, type MediaPopoutKind, type MediaPopoutTarget, type WindowLook } from "../../api";
import { applyLanguage, resolveLanguage } from "../../i18n";
import { applyRootLook, isCloseShortcut } from "../inspector/inspectorView";
import { Viewer } from "../screenshare/ScreenShareView";
import { seedScreenShares } from "../screenshare/sharesStore";
import { WatchTogetherView } from "../watchparty/WatchTogetherView";
import { setWatchParty, watchPartyOf } from "../watchparty/watchPartyStore";
import { InPopout, popIn, usePopouts } from "./popoutStore";

export function MediaPopoutWindow({ kind }: { kind: MediaPopoutKind }): ReactNode {
  const { t } = useTranslation("common");
  const target = usePopouts()[kind].target;
  useMainWindowLook();
  useSeed(kind, target);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!isCloseShortcut(event)) return;
      event.preventDefault();
      popIn(kind, t("media.popInFailed"));
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [kind, t]);

  return (
    <InPopout value>
      <main className="media-popout">
        {target !== null &&
          (kind === "watch" ? (
            <WatchTogetherView serverId={target.serverId} />
          ) : (
            target.session !== null && (
              <Viewer serverId={target.serverId} session={target.session} />
            )
          ))}
      </main>
      <LiveRegions />
    </InPopout>
  );
}

/** Theme and language of the main window, which shares them through the engine. */
function useMainWindowLook(): void {
  const [look, setLook] = useState<WindowLook | null>(null);

  useEffect(() => {
    const api = getApi();
    let updated = false;
    const stop = api.on("windowLookChanged", (changed) => {
      updated = true;
      setLook(changed.look);
    });
    api.windowLook().then(
      (shared) => {
        if (!updated && shared !== null) setLook(shared);
      },
      (error: unknown) => {
        console.error("Loading the main window's look failed", error);
      },
    );
    return stop;
  }, []);

  useEffect(() => {
    if (look === null) return;
    applyRootLook(document.documentElement, look);
    applyLanguage(resolveLanguage(null, [look.language])).catch((error: unknown) => {
      console.error("Switching the UI language failed", error);
    });
  }, [look]);
}

/**
 * The party or the shares as they are now: the stores only follow events, and this window opens
 * after the last one. An event that arrives first wins.
 */
function useSeed(kind: MediaPopoutKind, target: MediaPopoutTarget | null): void {
  const serverId = target?.serverId ?? null;
  useEffect(() => {
    if (serverId === null) return;
    const api = getApi();
    let current = true;
    if (kind === "watch") {
      api.watchPartyGet(serverId).then(
        (party) => {
          if (current && watchPartyOf(serverId) === null) setWatchParty(serverId, party);
        },
        (error: unknown) => {
          console.error("Loading the watch party failed", error);
        },
      );
    } else {
      api.screenShareList(serverId).then(
        (list) => {
          if (current) seedScreenShares(serverId, list);
        },
        (error: unknown) => {
          console.error("Reading the screen shares failed", error);
        },
      );
    }
    return () => {
      current = false;
    };
  }, [kind, serverId]);
}
