import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { Group } from "../components/Section";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Switch } from "../components/Switch";
import {
  effectivelyOn,
  type InGameGame,
  type InGameMode,
  type InGameOverlayApi,
  type InGameState,
} from "./inGameOverlay";
import { CornerSelect, SliderField } from "./OverlayFields";

function GameRow({
  game,
  mode,
  onToggle,
  onReset,
  onOptions,
}: {
  game: InGameGame;
  mode: InGameMode;
  onToggle: (enabled: boolean) => void;
  onReset: () => void;
  onOptions: (options: InGameGame["options"]) => void;
}) {
  const { t } = useTranslation("settings");
  const detailsId = useId();
  const [open, setOpen] = useState(false);
  const on = effectivelyOn(game, mode);
  return (
    <li className="overlay-settings__game">
      <div className="overlay-settings__game-head">
        {game.icon !== null && (
          <img className="overlay-settings__game-icon" src={game.icon} alt="" draggable={false} />
        )}
        <div className="overlay-settings__game-text">
          <Switch label={game.name} checked={on} onChange={onToggle} />
          <p className="settings-hint">
            {[
              game.detected ? t("overlay.inGame.running") : null,
              game.override === null
                ? t("overlay.inGame.followsDefault")
                : t("overlay.inGame.overridden"),
            ]
              .filter((part) => part !== null)
              .join(" · ")}
          </p>
          {game.blocked !== null && (
            <p className="overlay-settings__badge">
              <strong>{t("overlay.inGame.blocked")}</strong>{" "}
              {t("overlay.inGame.blockedReason", { reason: game.blocked.reason })}
            </p>
          )}
        </div>
      </div>
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          aria-expanded={open}
          aria-controls={detailsId}
          onClick={() => {
            setOpen(!open);
          }}
        >
          {t("overlay.inGame.options", { game: game.name })}
        </button>
        {game.override !== null && (
          <button type="button" className="settings-button" onClick={onReset}>
            {t("overlay.inGame.reset", { game: game.name })}
          </button>
        )}
      </div>
      {open && (
        <div id={detailsId} role="group" aria-label={game.name}>
          <CornerSelect
            label={t("overlay.corner.label")}
            value={game.options.corner}
            custom={false}
            onChange={(corner) => {
              onOptions({ ...game.options, corner });
            }}
          />
          <SliderField
            label={t("overlay.inGame.size")}
            value={game.options.size}
            min={50}
            max={200}
            step={10}
            unit="%"
            onChange={(size) => {
              onOptions({ ...game.options, size });
            }}
          />
          <SliderField
            label={t("overlay.opacity")}
            value={game.options.opacity}
            min={20}
            max={100}
            step={5}
            unit="%"
            onChange={(opacity) => {
              onOptions({ ...game.options, opacity });
            }}
          />
        </div>
      )}
    </li>
  );
}

/** In-game overlay (FR-1.46b): global Off/On plus per-game exceptions and the anti-cheat blocklist. */
export function InGameOverlayGroup({ api }: { api: InGameOverlayApi }) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<InGameState | null>(null);
  const [failed, setFailed] = useState(false);
  const [confirming, setConfirming] = useState<InGameGame | null>(null);

  useEffect(() => {
    let active = true;
    const unsubscribe = api.subscribe((changed) => {
      if (active) setState(changed);
    });
    api.state().then(
      (loaded) => {
        if (active) setState(loaded);
      },
      (e: unknown) => {
        console.error("Reading the in-game overlay state failed", e);
        if (active) setFailed(true);
      },
    );
    return () => {
      active = false;
      unsubscribe();
    };
  }, [api]);

  const run = (call: Promise<InGameState>) => {
    setFailed(false);
    call.then(setState, (e: unknown) => {
      console.error("Changing the in-game overlay failed", e);
      setFailed(true);
    });
  };

  if (state === null) {
    return (
      <Group title={t("overlay.inGame.title")}>
        <p className="settings-hint" role={failed ? "alert" : undefined}>
          {failed ? t("overlay.inGame.loadFailed") : t("loading")}
        </p>
      </Group>
    );
  }
  if (!state.supported) {
    return (
      <Group title={t("overlay.inGame.title")}>
        <p className="settings-hint" role="note">
          {t(state.explanationKey ?? "overlay.inGame.unsupported")}
        </p>
      </Group>
    );
  }

  const modeOptions: InGameMode[] = ["off", "on"];
  return (
    <Group title={t("overlay.inGame.title")}>
      <p className="settings-hint">{t("overlay.inGame.explanation")}</p>
      <div
        role="radiogroup"
        aria-label={t("overlay.inGame.mode")}
        className="overlay-settings__mode"
      >
        {modeOptions.map((mode) => (
          <button
            key={mode}
            type="button"
            role="radio"
            aria-checked={state.mode === mode}
            className={`settings-button${state.mode === mode ? " settings-button--primary" : ""}`}
            onClick={() => {
              if (state.mode !== mode) run(api.setMode(mode));
            }}
          >
            {t(`overlay.inGame.${mode}`)}
          </button>
        ))}
      </div>
      <p className="settings-hint">{t("overlay.inGame.antiCheat")}</p>
      {state.games.length === 0 ? (
        <p className="settings-hint">{t("overlay.inGame.noGames")}</p>
      ) : (
        <ul className="overlay-settings__games" aria-label={t("overlay.inGame.games")}>
          {state.games.map((game) => (
            <GameRow
              key={game.id}
              game={game}
              mode={state.mode}
              onToggle={(enabled) => {
                if (enabled && game.blocked !== null) setConfirming(game);
                else run(api.setGameOverride(game.id, enabled));
              }}
              onReset={() => {
                run(api.setGameOverride(game.id, null));
              }}
              onOptions={(options) => {
                run(api.setGameOptions(game.id, options));
              }}
            />
          ))}
        </ul>
      )}
      {failed && (
        <p className="settings-hint" role="alert">
          {t("overlay.inGame.saveFailed")}
        </p>
      )}
      {confirming !== null && (
        <ConfirmDialog
          title={t("overlay.inGame.confirmTitle", { game: confirming.name })}
          message={t("overlay.inGame.confirmMessage", {
            game: confirming.name,
            reason: confirming.blocked?.reason ?? "",
          })}
          confirmLabel={t("overlay.inGame.confirmEnable")}
          onCancel={() => {
            setConfirming(null);
          }}
          onConfirm={() => {
            run(api.setGameOverride(confirming.id, true));
            setConfirming(null);
          }}
        />
      )}
    </Group>
  );
}
