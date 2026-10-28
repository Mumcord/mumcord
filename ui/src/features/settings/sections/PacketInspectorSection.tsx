import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PacketCounter, type VoiceCounter } from "../../../api";
import { Select } from "../../../components/Select";
import { useServers as useConnectedServers } from "../../channels/serverStore";
import { setDevToolsUnlocked } from "../../inspector/devTools";
import { PacketTable } from "../../inspector/PacketTable";
import {
  exportJson,
  exportMarkdown,
  filterPackets,
  fromApi,
  mergePackets,
  NO_FILTER,
  packetKinds,
  PLUGIN_KINDS,
  type Names,
  type Packet,
  type PacketFilter,
} from "../../inspector/packets";
import { useServers as useSavedServers } from "../../servers/store";
import { Group, Section } from "../components/Section";
import { Switch } from "../components/Switch";
import { errorText } from "../errors";
import { pickSavePath } from "../fileDialogs";
import "../../inspector/inspector.css";

/** How often new packets are fetched; also batches them, so a burst never floods the list. */
export const POLL_MS = 250;

const ALL = "all";

type Status = { kind: "success" | "error"; text: string } | null;

/** Hidden packet inspector for debugging the plugin-data protocol (issue #60). */
export function PacketInspectorSection() {
  const { t } = useTranslation("settings");
  const [packets, setPackets] = useState<Packet[]>([]);
  const [counters, setCounters] = useState<PacketCounter[]>([]);
  const [voice, setVoice] = useState<VoiceCounter[]>([]);
  const [paused, setPaused] = useState(false);
  const [filter, setFilter] = useState<PacketFilter>(NO_FILTER);
  const [status, setStatus] = useState<Status>(null);
  const lastId = useRef<number | null>(null);
  const names = useNames();

  useEffect(() => {
    if (paused) return;
    let active = true;
    const poll = () => {
      getApi()
        .packetCapturePoll(lastId.current)
        .then(
          (capture) => {
            if (!active) return;
            const newest = capture.packets[capture.packets.length - 1];
            if (newest !== undefined) lastId.current = newest.id;
            setPackets((current) => mergePackets(current, capture.packets.map(fromApi)));
            setCounters(capture.counters);
            setVoice(capture.voice);
          },
          (error: unknown) => {
            console.error("Fetching captured packets failed", error);
          },
        );
    };
    poll();
    const timer = window.setInterval(poll, POLL_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [paused]);

  const shown = useMemo(() => filterPackets(packets, filter), [packets, filter]);
  const kinds = useMemo(() => packetKinds(packets), [packets]);
  const serverIds = useMemo(
    () => [...new Set([...packets.map((p) => p.serverId), ...voice.map((v) => v.serverId)])],
    [packets, voice],
  );

  const clear = () => {
    setPackets([]);
    setCounters([]);
    getApi()
      .packetCaptureClear()
      .catch((error: unknown) => {
        console.error("Clearing the captured packets failed", error);
      });
  };

  const save = async (format: "json" | "markdown") => {
    setStatus(null);
    try {
      const json = format === "json";
      const path = await pickSavePath(
        json ? "mumcord-packets.json" : "mumcord-packets.md",
        {
          name: t(json ? "inspector.fileTypeJson" : "inspector.fileTypeMarkdown"),
          extensions: [json ? "json" : "md"],
        },
        t(json ? "inspector.exportJson" : "inspector.exportMarkdown"),
      );
      if (path === null) return;
      const app = await getApi().appInfo();
      const info = { appVersion: app.version, os: app.os, exportedAtMs: Date.now() };
      const contents = json ? exportJson(shown, info, names) : exportMarkdown(shown, info, names);
      await getApi().packetCaptureExport(path, contents);
      setStatus({ kind: "success", text: t("inspector.exported") });
    } catch (error: unknown) {
      console.error("Exporting the captured packets failed", error);
      setStatus({ kind: "error", text: errorText(t, error) });
    }
  };

  const serverName = (id: number) => names.server(id) ?? `#${String(id)}`;
  const shownCounters = counters.filter(
    (c) => filter.serverId === null || c.serverId === filter.serverId,
  );
  const shownVoice = voice.filter(
    (v) => filter.serverId === null || v.serverId === filter.serverId,
  );

  return (
    <Section title={t("sections.packetInspector")}>
      <p className="settings-hint">{t("inspector.intro")}</p>
      <Switch
        label={t("inspector.devTools")}
        description={t("inspector.devToolsHint")}
        checked
        onChange={(on) => {
          if (!on) setDevToolsUnlocked(false);
        }}
      />
      <Group title={t("inspector.filters")}>
        <div className="pi-toolbar">
          <FilterSelect
            label={t("inspector.server")}
            value={filter.serverId === null ? ALL : String(filter.serverId)}
            options={[
              { value: ALL, label: t("inspector.allServers") },
              ...serverIds.map((id) => ({
                value: String(id),
                label: serverName(id),
              })),
            ]}
            onChange={(value) => {
              setFilter({ ...filter, serverId: value === ALL ? null : Number(value) });
            }}
          />
          <FilterSelect
            label={t("inspector.direction")}
            value={filter.direction ?? ALL}
            options={[
              { value: ALL, label: t("inspector.allDirections") },
              { value: "sent", label: t("inspector.sent") },
              { value: "received", label: t("inspector.received") },
            ]}
            onChange={(value) => {
              setFilter({
                ...filter,
                direction: value === "sent" || value === "received" ? value : null,
              });
            }}
          />
          <FilterSelect
            label={t("inspector.type")}
            value={filter.kind ?? ALL}
            options={[
              { value: ALL, label: t("inspector.allTypes") },
              ...kinds.map((kind) => ({ value: kind, label: kind })),
            ]}
            onChange={(value) => {
              setFilter({ ...filter, kind: value === ALL ? null : value });
            }}
          />
        </div>
        <div className="settings-actions">
          <button
            type="button"
            className="settings-button"
            aria-pressed={paused}
            onClick={() => {
              setPaused(!paused);
            }}
          >
            {t(paused ? "inspector.resume" : "inspector.pause")}
          </button>
          <button type="button" className="settings-button" onClick={clear}>
            {t("inspector.clear")}
          </button>
          <button type="button" className="settings-button" onClick={() => void save("json")}>
            {t("inspector.exportJson")}
          </button>
          <button type="button" className="settings-button" onClick={() => void save("markdown")}>
            {t("inspector.exportMarkdown")}
          </button>
        </div>
        <p className="settings-hint">{t("inspector.exportWarning")}</p>
        <p
          role="status"
          className={status?.kind === "error" ? "settings-error" : "settings-success"}
        >
          {status?.text}
        </p>
      </Group>
      <Group title={t("inspector.counters")}>
        {shownCounters.length === 0 && shownVoice.length === 0 ? (
          <p className="settings-hint">{t("inspector.noCounters")}</p>
        ) : (
          <ul className="pi-counters">
            {shownCounters.map((c) => (
              <li key={`${String(c.serverId)} ${c.kind} ${c.direction}`}>
                <span aria-hidden="true">{c.direction === "sent" ? "↑" : "↓"}</span>{" "}
                <span className="visually-hidden">
                  {t(c.direction === "sent" ? "inspector.sent" : "inspector.received")}
                </span>{" "}
                {c.kind in PLUGIN_KINDS && `${t(`inspector.kinds.${PLUGIN_KINDS[c.kind] ?? ""}`)} `}
                <code className="pi-kind-id">{c.kind}</code>: {c.count}
              </li>
            ))}
            {shownVoice.map((v) => (
              <li key={`voice ${String(v.serverId)}`}>
                {t("inspector.voice", { sent: v.sent, received: v.received })}
              </li>
            ))}
          </ul>
        )}
      </Group>
      <Group title={t("inspector.packets", { shown: shown.length, total: packets.length })}>
        {paused && <p className="settings-hint">{t("inspector.paused")}</p>}
        {shown.length === 0 ? (
          <p className="settings-hint">{t("inspector.empty")}</p>
        ) : (
          <PacketTable packets={shown} names={names} />
        )}
      </Group>
    </Section>
  );
}

function FilterSelect(props: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
}) {
  const id = useId();
  return (
    <div className="settings-control-row">
      <label htmlFor={id}>{props.label}</label>
      <Select id={id} value={props.value} options={props.options} onChange={props.onChange} />
    </div>
  );
}

/** Names of people and servers as the UI knows them right now. */
function useNames(): Names {
  const connected = useConnectedServers();
  const saved = useSavedServers().servers;
  return useMemo(
    () => ({
      user: (serverId, session) => connected.get(serverId)?.users.get(session)?.name ?? null,
      server: (serverId) => saved.find((s) => s.id === serverId)?.label ?? null,
    }),
    [connected, saved],
  );
}
