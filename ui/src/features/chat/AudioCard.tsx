/**
 * Inline player for a direct audio link (issue #260). Before the click the engine only reads
 * the start of the file (format and size); Play makes the engine fetch the whole file, checked
 * for size and format, and the player gets it as a blob, so the web view never contacts the
 * linked site. The audio plays on the app's output like any web view sound and never reaches
 * the microphone or the voice stream, which only carry what the audio engine captures.
 */
import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import type { AudioInfo } from "../../api";
import { getApi } from "../../api";
import { pickSavePath } from "../settings/fileDialogs";
import { dataUrlToBlob, formatSize, formatTime, waveformOf } from "./audioPlayer";
import { DismissButton } from "./cardButtons";
import { imageName, loadAudioFile, loadAudioInfo } from "./links";
import {
  AlertIcon,
  DownloadIcon,
  ExternalIcon,
  NoteIcon,
  PauseIcon,
  PlayIcon,
  VolumeIcon,
} from "./mediaIcons";
import { openInBrowser } from "./openLink";
import { useExclusivePlayback } from "./playback";
import "./mediaCards.css";

/** One volume for every audio card of the session: set once, it holds for the next file. */
let sharedVolume = 1;

type Phase = "idle" | "loading" | "ready" | "error";

interface AudioCardProps {
  url: string;
  tabIndex: number;
  onDismiss: () => void;
}

export function AudioCard({ url, tabIndex, onDismiss }: AudioCardProps) {
  const { t, i18n } = useTranslation("chat");
  const audio = useRef<HTMLAudioElement>(null);
  const [info, setInfo] = useState<AudioInfo | null | undefined>(undefined);
  const [phase, setPhase] = useState<Phase>("idle");
  const [src, setSrc] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState<number | null>(null);
  const [volume, setVolume] = useState(sharedVolume);
  const [muted, setMuted] = useState(false);
  const [peaks, setPeaks] = useState<number[] | null>(null);
  const [saveFailed, setSaveFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const name = imageName(url);
  const host = hostOf(url);

  useEffect(() => {
    let active = true;
    void loadAudioInfo(url, attempt > 0).then((loaded) => {
      if (!active) return;
      setInfo(loaded);
      if (loaded === null) setPhase("error");
    });
    return () => {
      active = false;
    };
  }, [url, attempt]);

  // The blob holds the whole file; give it back when the card goes or loads another.
  useEffect(
    () => () => {
      if (src !== null) URL.revokeObjectURL(src);
    },
    [src],
  );

  const pause = useCallback(() => {
    audio.current?.pause();
  }, []);
  useExclusivePlayback(playing, pause);

  useEffect(() => {
    const element = audio.current;
    if (element === null) return;
    element.volume = volume;
    element.muted = muted;
  }, [volume, muted, src]);

  const fail = useCallback((error: unknown) => {
    console.warn("Playing an audio file failed", error);
    setPlaying(false);
    setPhase("error");
  }, []);

  const start = async () => {
    if (src !== null) {
      await audio.current?.play().catch(fail);
      return;
    }
    setPhase("loading");
    const dataUrl = await loadAudioFile(url);
    const blob = dataUrl === null ? null : dataUrlToBlob(dataUrl);
    if (blob === null) {
      setPhase("error");
      return;
    }
    setSrc(URL.createObjectURL(blob));
    setPhase("ready");
    void waveformOf(blob).then(setPeaks);
  };

  // A fresh source starts as soon as the element has it: that click was Play.
  useEffect(() => {
    if (src !== null) audio.current?.play().catch(fail);
  }, [src, fail]);

  const retry = () => {
    setSrc(null);
    setPeaks(null);
    setTime(0);
    setDuration(null);
    setInfo(undefined);
    setPhase("idle");
    setAttempt((n) => n + 1);
  };

  const save = async () => {
    setSaveFailed(false);
    const extension = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? "audio";
    try {
      const path = await pickSavePath(
        name,
        { name: t("audio.fileType"), extensions: [extension] },
        t("audio.saveTitle"),
      );
      if (path !== null) await getApi().audioSave(url, path);
    } catch (error: unknown) {
      console.error("Saving an audio file failed", error);
      setSaveFailed(true);
    }
  };
  const error = phase === "error";
  const length = duration;
  const progress = length !== null && length > 0 ? time / length : 0;
  const status = playing
    ? t("audio.playing")
    : phase === "loading"
      ? t("audio.loading")
      : t("audio.file");
  const facts = [status, host];
  if (!playing && !error && info !== undefined && info !== null && info.size !== null) {
    facts.push(formatSize(info.size, i18n.language));
  }

  return (
    <article
      className={`audio-card${playing ? " audio-card--playing" : ""}${error ? " audio-card--error" : ""}`}
      aria-label={t("audio.card", { name })}
      aria-busy={phase === "loading" || info === undefined}
    >
      <div className="audio-card__head">
        <span className="audio-card__icon" aria-hidden="true">
          <NoteIcon size={20} />
        </span>
        <div className="audio-card__text">
          <p className="audio-card__name">{name}</p>
          <p className="audio-card__facts">{facts.join(" · ")}</p>
        </div>
        {playing && (
          <span className="audio-card__eq" aria-hidden="true">
            <span />
            <span />
            <span />
            <span />
          </span>
        )}
      </div>
      {error ? (
        <div className="audio-card__error" role="alert">
          <p className="audio-card__error-title">
            <AlertIcon size={16} />
            {t("audio.errorTitle")}
          </p>
          <p className="audio-card__error-text">{t("audio.errorText")}</p>
          <div className="audio-card__error-actions">
            <button type="button" className="media-pill" tabIndex={tabIndex} onClick={retry}>
              {t("audio.retry")}
            </button>
            <button
              type="button"
              className="media-pill"
              tabIndex={tabIndex}
              onClick={() => {
                openInBrowser(url);
              }}
            >
              {t("preview.open")}
            </button>
          </div>
        </div>
      ) : (
        <>
          {peaks !== null && (
            <div className="audio-card__wave" aria-hidden="true">
              {peaks.map((peak, i) => (
                <span
                  key={i}
                  className={i / peaks.length < progress ? "audio-card__bar--played" : undefined}
                  style={{ height: `${String(Math.max(12, Math.round(peak * 100)))}%` }}
                />
              ))}
            </div>
          )}
          <div className="audio-card__controls">
            <button
              type="button"
              className="audio-card__play"
              tabIndex={tabIndex}
              disabled={info === undefined || phase === "loading"}
              aria-label={t(playing ? "audio.pause" : "audio.play", { name })}
              data-tooltip={t(playing ? "audio.pauseShort" : "audio.playShort")}
              onClick={() => {
                if (playing) pause();
                else void start();
              }}
            >
              {playing ? <PauseIcon size={16} /> : <PlayIcon size={16} />}
            </button>
            <span className="audio-card__time">{formatTime(time)}</span>
            <input
              type="range"
              className="audio-card__seek"
              style={{ "--progress": `${String(progress * 100)}%` } as CSSProperties}
              tabIndex={tabIndex}
              min={0}
              max={length ?? 0}
              step={0.1}
              value={time}
              disabled={length === null}
              aria-label={t("audio.seek")}
              aria-valuetext={t("audio.position", {
                time: formatTime(time),
                length: formatTime(length),
              })}
              onChange={(event) => {
                const next = Number(event.target.value);
                if (audio.current !== null) audio.current.currentTime = next;
                setTime(next);
              }}
            />
            <span className="audio-card__time">{formatTime(length)}</span>
            <button
              type="button"
              className="audio-card__icon-button"
              tabIndex={tabIndex}
              aria-pressed={muted}
              aria-label={t("audio.mute")}
              data-tooltip={t("audio.mute")}
              onClick={() => {
                setMuted(!muted);
              }}
            >
              <VolumeIcon muted={muted || volume === 0} size={18} />
            </button>
            <input
              type="range"
              className="audio-card__volume"
              style={{ "--progress": `${String(volume * 100)}%` } as CSSProperties}
              tabIndex={tabIndex}
              min={0}
              max={1}
              step={0.05}
              value={volume}
              aria-label={t("audio.volume")}
              aria-valuetext={`${String(Math.round(volume * 100))} %`}
              onChange={(event) => {
                const next = Number(event.target.value);
                sharedVolume = next;
                setVolume(next);
                setMuted(false);
              }}
            />
            <button
              type="button"
              className="audio-card__icon-button"
              tabIndex={tabIndex}
              aria-label={t("audio.open", { name })}
              data-tooltip={t("preview.open")}
              onClick={() => {
                openInBrowser(url);
              }}
            >
              <ExternalIcon size={18} />
            </button>
            <button
              type="button"
              className="audio-card__icon-button"
              tabIndex={tabIndex}
              disabled={info === undefined}
              aria-label={t("audio.save", { name })}
              data-tooltip={t("audio.saveShort")}
              onClick={() => {
                void save();
              }}
            >
              <DownloadIcon size={18} />
            </button>
          </div>
          {saveFailed && (
            <p className="audio-card__save-failed" role="alert">
              {t("audio.saveFailed")}
            </p>
          )}
        </>
      )}
      {src !== null && (
        // A linked music file has no captions to offer; the element is only the engine behind
        // the card's own labelled controls.
        // eslint-disable-next-line jsx-a11y/media-has-caption
        <audio
          ref={audio}
          src={src}
          preload="auto"
          onPlay={() => {
            setPlaying(true);
          }}
          onPause={() => {
            setPlaying(false);
          }}
          onEnded={() => {
            setPlaying(false);
          }}
          onTimeUpdate={(event) => {
            setTime(event.currentTarget.currentTime);
          }}
          onLoadedMetadata={(event) => {
            const length = event.currentTarget.duration;
            setDuration(Number.isFinite(length) ? length : null);
          }}
          onError={() => {
            fail(audio.current?.error ?? "media error");
          }}
        />
      )}
      <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
    </article>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}
