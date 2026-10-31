import { useEffect, useId, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ChannelAudioProfile, type Settings } from "../../api";
import { useServerState } from "../channels";
import { useServers } from "../servers";
import { Dialog } from "../servers/Dialog";
import { displayName } from "../servers/format";
import { Switch } from "../settings/components/Switch";
import { BITRATE_MIN_KBPS, BITRATE_OPUS_MAX_KBPS } from "../settings/sections/BitrateSlider";
import { saveChannelAudioProfile, useChannelAudioProfile } from "./channelAudioStore";
import { MUSIC_BITRATE_KBPS, PRESET_IDS, PRESETS, presetOf, type PresetId } from "./model";
import "../settings/settings.css";
import "./channel-audio.css";

const PRESET_ICON: Record<PresetId, string | null> = { voice: null, music: "♪", jam: "⚡" };

interface ChannelAudioDialogProps {
  /** Channel of the shown server. */
  channelId: number;
  onClose: () => void;
}

/**
 * "Audio for this channel" (issue #171, FR-3.62–FR-3.64): presets, Music quality, low-latency mode
 * and the bitrate cap for one channel of the shown server. Every value can follow the global
 * setting ("default"). Saved locally; applies whenever the user is in the channel.
 */
export function ChannelAudioDialog({ channelId, onClose }: ChannelAudioDialogProps) {
  const { t } = useTranslation("channelheader");
  const server = useServerState();
  const { servers } = useServers();
  const { serverId } = server;
  const saved = useChannelAudioProfile(serverId, channelId);
  // `null` until the user edits: the stored profile, which can arrive after opening, shows.
  const [edited, setEdited] = useState<ChannelAudioProfile | null>(null);
  const draft = edited ?? saved ?? PRESETS.voice;
  const [global, setGlobal] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const noteId = useId();
  const bitrateId = useId();

  useEffect(() => {
    let live = true;
    getApi()
      .settingsGet()
      .then(
        (settings) => {
          if (live) setGlobal(settings);
        },
        (error: unknown) => {
          console.error("Loading the settings failed", error);
        },
      );
    return () => {
      live = false;
    };
  }, []);

  const channel = server.channels.get(channelId);
  if (!channel || serverId === null) return null;

  const edit = (patch: Partial<ChannelAudioProfile>) => {
    setFailed(false);
    setEdited({ ...draft, ...patch });
  };

  const store = (profile: ChannelAudioProfile | null) => {
    setBusy(true);
    saveChannelAudioProfile(serverId, channelId, profile).then(
      () => {
        onClose();
      },
      (error: unknown) => {
        console.error("Saving the channel audio profile failed", error);
        setBusy(false);
        setFailed(true);
      },
    );
  };

  const savedServer = servers.find((s) => s.id === serverId);
  const onOff = (on: boolean) => (on ? t("audio.on") : t("audio.off"));
  const globalLowLatency = global?.lowLatency ?? false;
  const globalMusic = global?.musicQuality ?? false;
  const lowLatency = draft.lowLatency ?? globalLowLatency;
  const music = draft.musicQuality ?? globalMusic;
  const globalCap = global === null ? null : (global.bitrateKbps ?? BITRATE_OPUS_MAX_KBPS);
  const bitrate = draft.bitrateKbps ?? globalCap ?? BITRATE_OPUS_MAX_KBPS;
  const kbpsText = (kbps: number) =>
    kbps >= BITRATE_OPUS_MAX_KBPS ? t("audio.bitrateMax") : t("audio.bitrateValue", { kbps });
  const maxBandwidth = server.info?.maxBandwidth ?? null;
  const limitKbps =
    maxBandwidth !== null && maxBandwidth > 0 ? Math.round(maxBandwidth / 1000) : null;
  const span = BITRATE_OPUS_MAX_KBPS - BITRATE_MIN_KBPS;
  const limitAt =
    limitKbps === null
      ? null
      : (Math.min(Math.max(limitKbps, BITRATE_MIN_KBPS), BITRATE_OPUS_MAX_KBPS) -
          BITRATE_MIN_KBPS) /
        span;
  const preset = presetOf(draft);

  /** "Default (off)" while a switch follows the global setting, else a way back to it. */
  const defaultLine = (value: boolean | null, globalValue: boolean, reset: () => void) =>
    value === null ? (
      <p className="channel-audio__default">
        {t("audio.followsDefault", { value: onOff(globalValue) })}
      </p>
    ) : (
      <p className="channel-audio__default">
        <button type="button" className="channel-audio__link" onClick={reset}>
          {t("audio.useDefault")}
        </button>{" "}
        ({onOff(globalValue)})
      </p>
    );

  return (
    <Dialog
      className="channel-audio"
      title={t("audio.title")}
      onClose={onClose}
      describedBy={noteId}
    >
      <p className="channel-audio__subtitle">
        <span aria-hidden="true">#</span>
        {[channel.name, savedServer ? displayName(savedServer) : ""].filter(Boolean).join(" · ")}
      </p>
      <fieldset className="channel-audio__presets">
        <legend className="visually-hidden">{t("audio.presets")}</legend>
        {PRESET_IDS.map((id) => (
          <label
            key={id}
            className={`channel-audio__preset${preset === id ? " channel-audio__preset--on" : ""}`}
          >
            <input
              type="radio"
              className="visually-hidden"
              name={`channel-audio-preset-${String(channelId)}`}
              checked={preset === id}
              onChange={() => {
                edit(PRESETS[id]);
              }}
            />
            <span className="channel-audio__preset-name">
              {PRESET_ICON[id] !== null && <span aria-hidden="true">{PRESET_ICON[id]} </span>}
              {t(`audio.preset.${id}`)}
            </span>
            <span className="channel-audio__preset-hint">
              {t(`audio.preset.${id}Hint`, { kbps: MUSIC_BITRATE_KBPS })}
            </span>
          </label>
        ))}
      </fieldset>
      <section className="channel-audio__row">
        <Switch
          label={t("audio.musicQuality")}
          description={lowLatency ? t("audio.musicIncluded") : t("audio.musicQualityHint")}
          checked={lowLatency || music}
          disabled={lowLatency}
          onChange={(on) => {
            edit({ musicQuality: on });
          }}
        />
        {!lowLatency &&
          defaultLine(draft.musicQuality, globalMusic, () => {
            edit({ musicQuality: null });
          })}
      </section>
      <section className="channel-audio__row">
        <Switch
          label={t("audio.lowLatency")}
          badge={t("audio.beta")}
          description={t("audio.lowLatencyHint")}
          checked={lowLatency}
          onChange={(on) => {
            edit({ lowLatency: on });
          }}
        />
        {defaultLine(draft.lowLatency, globalLowLatency, () => {
          edit({ lowLatency: null });
        })}
      </section>
      <section className="channel-audio__row channel-audio__bitrate">
        <div className="channel-audio__bitrate-line">
          <label htmlFor={bitrateId}>{t("audio.bitrate")}</label>
          <div
            className="channel-audio__track"
            style={limitAt === null ? undefined : ({ "--limit-at": limitAt } as CSSProperties)}
          >
            <input
              id={bitrateId}
              type="range"
              min={BITRATE_MIN_KBPS}
              max={BITRATE_OPUS_MAX_KBPS}
              step={1}
              value={bitrate}
              aria-valuetext={
                draft.bitrateKbps === null
                  ? t("audio.bitrateDefault", { value: kbpsText(bitrate) })
                  : kbpsText(bitrate)
              }
              aria-describedby={`${bitrateId}-scale`}
              onChange={(event) => {
                edit({ bitrateKbps: Number(event.target.value) });
              }}
            />
            {limitAt !== null && <span className="channel-audio__limit" aria-hidden="true" />}
          </div>
          <output htmlFor={bitrateId} className="channel-audio__bitrate-value">
            {kbpsText(bitrate)}
          </output>
        </div>
        <p id={`${bitrateId}-scale`} className="channel-audio__scale">
          <span>{BITRATE_MIN_KBPS}</span>
          <span>
            {limitKbps === null
              ? t("audio.noServerLimit")
              : t("audio.serverLimit", { kbps: limitKbps })}
            {" · "}
            {draft.bitrateKbps === null ? (
              t("audio.followsDefault", {
                value: globalCap === null ? "…" : kbpsText(globalCap),
              })
            ) : (
              <>
                <button
                  type="button"
                  className="channel-audio__link"
                  onClick={() => {
                    edit({ bitrateKbps: null });
                  }}
                >
                  {t("audio.useDefault")}
                </button>{" "}
                ({globalCap === null ? "…" : kbpsText(globalCap)})
              </>
            )}
          </span>
        </p>
      </section>
      <p id={noteId} className="channel-audio__note">
        {t("audio.note")}
      </p>
      {failed && (
        <p className="channel-audio__error" role="alert">
          {t("audio.saveFailed")}
        </p>
      )}
      <div className="channel-audio__footer">
        {saved !== null && (
          <button
            type="button"
            className="channel-audio__link channel-audio__remove"
            disabled={busy}
            onClick={() => {
              store(null);
            }}
          >
            {t("audio.remove")}
          </button>
        )}
        <button type="button" className="button" onClick={onClose}>
          {t("audio.cancel")}
        </button>
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => {
            store(draft);
          }}
        >
          {t("audio.save")}
        </button>
      </div>
    </Dialog>
  );
}
