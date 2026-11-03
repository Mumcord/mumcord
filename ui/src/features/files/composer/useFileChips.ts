/**
 * The files waiting in the attachment tray of the message input: adding by path (picker or
 * native drop), live progress from the engine, removing and sending as one offer per file.
 */
import { useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { EngineError, getApi } from "../../../api";
import { ViewContext } from "../../../app/view";
import type { FileChip, FilesUnavailable } from "../../../bindings/commands";
import { shownServerId } from "../../channels/serverStore";
import { addSentMessage } from "../../chat/chatStore";
import { isBusy, MAX_CHIPS } from "./chipView";

export interface FileChips {
  chips: readonly FileChip[];
  /** Why sharing is impossible on this server right now; `null` when it works. */
  unavailable: FilesUnavailable | null;
  error: string | null;
  /** True while a chip is zipping or being checked: sending waits. */
  busy: boolean;
  /** At least one chip can be sent. */
  hasSendable: boolean;
  addPaths: (paths: readonly string[]) => Promise<void>;
  remove: (chipId: number) => void;
  /** Shows a message under the tray (e.g. a refused drop). */
  fail: (message: string) => void;
  /** Posts the ready chips into the channel; failures stay in the tray. */
  send: (channelId: number) => Promise<void>;
}

export function useFileChips(enabled: boolean): FileChips {
  const { t } = useTranslation("files");
  const [chips, setChips] = useState<readonly FileChip[]>([]);
  const [unavailable, setUnavailable] = useState<FilesUnavailable | null>(null);
  const [error, setError] = useState<string | null>(null);
  const chipsRef = useRef(chips);
  useEffect(() => {
    chipsRef.current = chips;
  });

  useEffect(() => {
    return getApi().on("fileChipChanged", ({ chip }) => {
      setChips((list) => {
        if (chip.state.kind === "sent") return list.filter((item) => item.id !== chip.id);
        // Events for chips this tray did not add (other windows) are ignored.
        return list.some((item) => item.id === chip.id)
          ? list.map((item) => (item.id === chip.id ? chip : item))
          : list;
      });
    });
  }, []);

  const serverId = shownServerId();
  // The engine sends no event when settings change: opening and closing the settings reads the
  // availability again (the peer-to-peer mode lives there).
  const overlay = useContext(ViewContext)?.overlay ?? null;
  useEffect(() => {
    if (!enabled || overlay !== null) return;
    let alive = true;
    const refresh = () => {
      getApi()
        .filesAvailability(serverId)
        .then(
          (reason) => {
            if (alive) setUnavailable(reason);
          },
          (err: unknown) => {
            console.error("reading file sharing availability failed", err);
          },
        );
    };
    refresh();
    // The settings can also change in a separate window; looking again on return covers it.
    window.addEventListener("focus", refresh);
    const stopRelay = getApi().on("relayChanged", (event) => {
      if (event.serverId === serverId) refresh();
    });
    return () => {
      alive = false;
      window.removeEventListener("focus", refresh);
      stopRelay();
    };
  }, [enabled, serverId, overlay]);

  const addPaths = useCallback(
    async (paths: readonly string[]) => {
      setError(null);
      // Counted here, not read back from state: a render committed before an add resolved can
      // still hand the effect an older list, and building on that list lost dropped files.
      let count = chipsRef.current.length;
      for (const path of paths) {
        if (count >= MAX_CHIPS) {
          setError(t("composer.tooMany"));
          return;
        }
        try {
          const chip = await getApi().filesChipAdd(path);
          count += 1;
          setChips((list) => [...list, chip]);
        } catch (err) {
          console.error("adding a file failed", err);
          setError(err instanceof EngineError ? err.message : t("composer.unreadable", { path }));
        }
      }
    },
    [t],
  );

  const remove = useCallback((chipId: number) => {
    setChips((list) => list.filter((item) => item.id !== chipId));
    setError(null);
    getApi()
      .filesChipRemove(chipId)
      .catch((err: unknown) => {
        console.error("removing a file failed", err);
      });
  }, []);

  const send = useCallback(async (channelId: number) => {
    const server = shownServerId();
    const ids = chipsRef.current.filter((chip) => chip.state.kind === "ready").map((c) => c.id);
    if (ids.length === 0) return;
    setError(null);
    try {
      // Like every own message, the posted offers come back instead of as events.
      const posted = await getApi().filesSend(server, channelId, ids);
      for (const message of posted) addSentMessage(message, server);
    } catch (err) {
      console.error("sharing files failed", err);
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const busy = useMemo(() => chips.some(isBusy), [chips]);
  const hasSendable = useMemo(() => chips.some((chip) => chip.state.kind === "ready"), [chips]);
  return { chips, unavailable, error, busy, hasSendable, addPaths, remove, fail: setError, send };
}
