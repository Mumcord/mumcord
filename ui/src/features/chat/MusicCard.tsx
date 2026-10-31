/**
 * Music card under a message (issue #262): cover, service badge, title and artist from the
 * service's oEmbed or page metadata, and the service's official embed, which loads only after
 * Play. Services without a documented embed get the same card without a player. Every text is
 * plain text from the engine; the cover is a `data:` URL the engine re-encoded.
 */
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LinkPreview, MusicEmbed, MusicService } from "../../api";
import { DismissButton } from "./cardButtons";
import { musicPlayer, safeImage } from "./links";
import { ExternalIcon, GlobeIcon, NoteIcon, PlayIcon, StopIcon } from "./mediaIcons";
import { openInBrowser } from "./openLink";
import { PagePlayer } from "./PagePlayer";
import { useExclusivePlayback } from "./playback";
import "./mediaCards.css";

/** Service names are brand names and stay untranslated; only "other" is a description. */
const SERVICE_NAMES: Record<Exclude<MusicService, "other">, string> = {
  spotify: "Spotify",
  soundcloud: "SoundCloud",
  youtubeMusic: "YouTube Music",
  appleMusic: "Apple Music",
  tidal: "Tidal",
  deezer: "Deezer",
  bandcamp: "Bandcamp",
};

/** Height of the services' fixed-size widgets: compact for one track, tall for lists. */
function embedHeight(music: MusicEmbed): number | null {
  const single = music.kind === "track" || music.kind === "episode";
  if (music.service === "spotify") return single ? 152 : 352;
  if (music.service === "soundcloud") return single ? 166 : 450;
  return null;
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

interface MusicCardProps {
  preview: LinkPreview;
  music: MusicEmbed;
  tabIndex: number;
  onDismiss: () => void;
}

export function MusicCard({ preview, music, tabIndex, onDismiss }: MusicCardProps) {
  const { t } = useTranslation("chat");
  const [playing, setPlaying] = useState(false);
  const stop = useCallback(() => {
    setPlaying(false);
  }, []);
  useExclusivePlayback(playing, stop);
  const player = musicPlayer(music);
  const service = music.service === "other" ? t("music.other") : SERVICE_NAMES[music.service];
  const title =
    preview.title ?? (music.service === "other" ? service : t("music.title", { service }));
  const kind = t(`preview.kind.${music.kind}`);
  const details =
    music.artist !== null ? `${music.artist} · ${kind}` : (preview.description ?? kind);
  const cover = safeImage(preview.image);
  const host = hostOf(player?.url ?? preview.url);
  const height = embedHeight(music);
  const openLabel = music.service === "other" ? t("preview.open") : t("music.openIn", { service });
  const note =
    player === null ? t("music.noPlayer") : playing ? t("music.output") : t("music.limits");
  return (
    <article
      className={`music-card music-card--${music.service}`}
      aria-label={t("music.card", { service, title })}
    >
      <div className="music-card__head">
        {cover !== null ? (
          <img className="music-card__cover" src={cover} alt="" />
        ) : (
          <span className="music-card__cover music-card__cover--empty" aria-hidden="true">
            <NoteIcon size={32} />
          </span>
        )}
        <div className="music-card__text">
          <span className="music-card__badge">{service}</span>
          <p className="music-card__title">{title}</p>
          <p className="music-card__details">{details}</p>
          <p className="music-card__third-party">
            <GlobeIcon size={12} />
            {t("music.thirdParty", { host })}
          </p>
        </div>
      </div>
      {playing && player !== null && (
        <div className="music-card__embed">
          <p className="music-card__embed-bar">
            <span>{t("music.embedded", { service, host })}</span>
            <span>{t("music.thirdPartyShort")}</span>
          </p>
          {player.via === "page" ? (
            <PagePlayer
              playerUrl={player.url}
              title={t("music.player", { service, title })}
              onFailed={() => {
                setPlaying(false);
                openInBrowser(preview.url);
              }}
            />
          ) : (
            <iframe
              className="music-card__player"
              style={height === null ? undefined : { height }}
              src={player.url}
              title={t("music.player", { service, title })}
              allow="autoplay; encrypted-media"
              referrerPolicy="strict-origin-when-cross-origin"
              sandbox="allow-scripts allow-same-origin allow-presentation"
            />
          )}
        </div>
      )}
      <div className="music-card__actions">
        {player !== null && (
          <button
            type="button"
            className="music-card__play"
            tabIndex={tabIndex}
            aria-label={t(playing ? "music.stopLabel" : "music.playLabel", { service, title })}
            onClick={() => {
              setPlaying(!playing);
            }}
          >
            {playing ? <StopIcon size={12} /> : <PlayIcon size={12} />}
            {t(playing ? "music.stop" : "music.play")}
          </button>
        )}
        <button
          type="button"
          className="music-card__open"
          tabIndex={tabIndex}
          aria-label={`${openLabel}: ${title}`}
          onClick={() => {
            openInBrowser(preview.url);
          }}
        >
          <ExternalIcon size={13} />
          {openLabel}
        </button>
        <span className="music-card__note">{note}</span>
      </div>
      <DismissButton onDismiss={onDismiss} tabIndex={tabIndex} />
    </article>
  );
}
