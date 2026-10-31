import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, toEngineError, type WhisperReceivers, type WhisperTarget } from "../../api";
import { channelPath } from "../channelheader/channelModel";
import { useServerStateOf, useShownServerId, useVoiceServerId } from "../channels/serverStore";
import { Group } from "../settings/components/Section";
import { errorText } from "../settings/errors";
import { keyCombo } from "../settings/pttKey";
import {
  clearWhisperDraft,
  saveWhisperTargets,
  useWhisperDraft,
  useWhisperSetup,
  whisperKind,
  type WhisperDraft,
} from "./whisperStore";
import { WhisperKindIcon } from "./WhisperKindIcon";
import "./whisper.css";

const newId = () => `t-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

const EMPTY_TARGET = (): WhisperTarget => ({
  id: newId(),
  name: "",
  receivers: { kind: "people", names: [] },
  hotkey: null,
});

/**
 * Settings → Voice → "Whisper and shout targets" (issue #67, mockup `67-whisper-setup`): the saved
 * targets on the left, the editor of the selected one on the right. People and channels come from
 * the voice server (else the shown one); targets are global and found again by name.
 */
export function WhisperTargetsGroup() {
  const { t } = useTranslation("settings");
  const setup = useWhisperSetup();
  const draft = useWhisperDraft();
  const targets = useMemo(() => setup?.targets ?? [], [setup]);
  const [editing, setEditing] = useState<WhisperTarget | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const groupRef = useRef<HTMLDivElement>(null);

  // A "Whisper to…" from a menu opens its draft as a new target (adjusted while rendering, so
  // the editor never shows the previous target first).
  const [openedDraft, setOpenedDraft] = useState<WhisperDraft | null>(null);
  if (draft !== null && draft !== openedDraft) {
    setOpenedDraft(draft);
    setEditing({ ...EMPTY_TARGET(), name: draft.name, receivers: draft.receivers });
    setStatus(null);
    setProblem(null);
  }
  useEffect(() => {
    if (draft === null) return;
    clearWhisperDraft();
    groupRef.current?.scrollIntoView({ block: "start" });
    groupRef.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [draft]);

  // Without a choice, the first saved target is shown.
  const selected = editing ?? targets[0] ?? null;
  const isNew = selected !== null && !targets.some((target) => target.id === selected.id);

  const persist = async (next: WhisperTarget[], message: string | null) => {
    try {
      await saveWhisperTargets(next);
      setProblem(null);
      setStatus(message);
      return true;
    } catch (error: unknown) {
      const engineError = toEngineError(error);
      setStatus(null);
      setProblem(
        engineError.code === "invalidInput" ? t("voice.whisper.keyTaken") : errorText(t, error),
      );
      return false;
    }
  };

  const save = async (target: WhisperTarget) => {
    if (target.name.trim() === "") {
      setProblem(t("voice.whisper.nameMissing"));
      return;
    }
    const empty =
      target.receivers.kind === "people"
        ? target.receivers.names.length === 0
        : target.receivers.path.length === 0;
    if (empty) {
      setProblem(t("voice.whisper.receiversMissing"));
      return;
    }
    const next = isNew
      ? [...targets, target]
      : targets.map((existing) => (existing.id === target.id ? target : existing));
    if (await persist(next, t("voice.whisper.savedStatus"))) setEditing(target);
  };

  const remove = async (target: WhisperTarget) => {
    if (isNew) {
      setEditing(null);
      return;
    }
    if (
      await persist(
        targets.filter((existing) => existing.id !== target.id),
        null,
      )
    )
      setEditing(null);
  };

  return (
    <div ref={groupRef}>
      <Group title={t("voice.whisper.title")}>
        <div className="whisper-setup">
          <div className="whisper-setup__list">
            <h4 className="whisper-setup__heading" id="whisper-saved">
              {t("voice.whisper.saved")}
            </h4>
            <ul className="whisper-setup__targets" aria-labelledby="whisper-saved">
              {targets.map((target) => (
                <li key={target.id}>
                  <TargetRow
                    target={target}
                    selected={selected?.id === target.id}
                    failedKey={setup?.failedKeys.includes(target.id) ?? false}
                    onSelect={() => {
                      setEditing(target);
                      setStatus(null);
                      setProblem(null);
                    }}
                  />
                </li>
              ))}
              {isNew && (
                <li>
                  <TargetRow target={selected} selected failedKey={false} onSelect={() => {}} />
                </li>
              )}
              <li>
                <button
                  type="button"
                  className="whisper-setup__new"
                  onClick={() => {
                    setEditing(EMPTY_TARGET());
                    setStatus(null);
                    setProblem(null);
                  }}
                >
                  + {t("voice.whisper.new")}
                </button>
              </li>
            </ul>
            {targets.length === 0 && !isNew && (
              <p className="settings-hint">{t("voice.whisper.empty")}</p>
            )}
            <p className="settings-hint">{t("voice.whisper.hint")}</p>
          </div>
          {selected !== null && (
            <TargetEditor
              key={selected.id}
              target={selected}
              keysUnavailable={setup?.keysUnavailable ?? null}
              failedKey={setup?.failedKeys.includes(selected.id) ?? false}
              onSave={(target) => void save(target)}
              onDelete={(target) => void remove(target)}
            />
          )}
        </div>
        <p role="status" className="settings-hint">
          {status}
        </p>
        <p role="alert" className="settings-error">
          {problem}
        </p>
      </Group>
    </div>
  );
}

function TargetRow({
  target,
  selected,
  failedKey,
  onSelect,
}: {
  target: WhisperTarget;
  selected: boolean;
  failedKey: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation("settings");
  const receivers = target.receivers;
  const summary =
    receivers.kind === "people"
      ? t("voice.whisper.summaryPeople", { count: receivers.names.length })
      : receivers.children
        ? t("voice.whisper.summaryChannelTree")
        : t("voice.whisper.summaryChannel");
  return (
    <button
      type="button"
      className="whisper-setup__target"
      aria-current={selected ? "true" : undefined}
      onClick={onSelect}
    >
      <WhisperKindIcon kind={whisperKind(receivers)} />
      <span className="whisper-setup__target-text">
        <span className="whisper-setup__target-name">{target.name || t("voice.whisper.new")}</span>
        <span className="whisper-setup__target-summary">{summary}</span>
      </span>
      <kbd className="settings-kbd whisper-setup__key" data-failed={failedKey || undefined}>
        {target.hotkey ?? t("voice.whisper.keyNotSet")}
      </kbd>
    </button>
  );
}

function TargetEditor({
  target,
  keysUnavailable,
  failedKey,
  onSave,
  onDelete,
}: {
  target: WhisperTarget;
  keysUnavailable: string | null;
  failedKey: boolean;
  onSave: (target: WhisperTarget) => void;
  onDelete: (target: WhisperTarget) => void;
}) {
  const { t } = useTranslation("settings");
  const id = useId();
  const [name, setName] = useState(target.name);
  const [receivers, setReceivers] = useState<WhisperReceivers>(target.receivers);
  const [hotkey, setHotkey] = useState<string | null>(target.hotkey);
  const voice = useVoiceServerId();
  const shown = useShownServerId();
  const server = useServerStateOf(voice ?? shown);

  const people = useMemo(
    () =>
      [...server.users.values()]
        .filter((user) => user.session !== server.selfSession)
        .map((user) => user.name)
        .sort((a, b) => a.localeCompare(b)),
    [server.users, server.selfSession],
  );
  const channels = useMemo(
    () =>
      [...server.channels.values()]
        .filter((channel) => channel.parentId !== null)
        .map((channel) => channelPath(server.channels, channel.id))
        .sort((a, b) => a.join("/").localeCompare(b.join("/"))),
    [server.channels],
  );

  const names = receivers.kind === "people" ? receivers.names : [];
  // Saved people who are not online right now stay in the list, so they can be removed.
  const peopleChoices = [...people, ...names.filter((n) => !people.includes(n))];
  const pathKey = receivers.kind === "channel" ? receivers.path.join("/") : "";
  const channelKnown = channels.some((path) => path.join("/") === pathKey);

  const setKind = (kind: WhisperReceivers["kind"]) => {
    if (kind === receivers.kind) return;
    setReceivers(
      kind === "people"
        ? { kind: "people", names: [] }
        : { kind: "channel", path: channels[0] ?? [], children: false, links: false, group: null },
    );
  };

  return (
    <form
      className="whisper-setup__editor"
      aria-labelledby={`${id}-edit`}
      onSubmit={(event) => {
        event.preventDefault();
        onSave({ ...target, name, receivers, hotkey });
      }}
    >
      <h4 className="whisper-setup__heading" id={`${id}-edit`}>
        {t("voice.whisper.edit")}
      </h4>
      <label className="visually-hidden" htmlFor={`${id}-name`}>
        {t("voice.whisper.name")}
      </label>
      <input
        id={`${id}-name`}
        className="settings-input whisper-setup__name"
        value={name}
        maxLength={64}
        placeholder={t("voice.whisper.name")}
        onChange={(event) => {
          setName(event.target.value);
        }}
      />
      <fieldset className="whisper-setup__kinds">
        <legend className="visually-hidden">{t("voice.whisper.receiversLegend")}</legend>
        <div className="whisper-setup__kind">
          <input
            id={`${id}-people`}
            type="radio"
            name={`${id}-kind`}
            checked={receivers.kind === "people"}
            aria-describedby={`${id}-people-hint`}
            onChange={() => {
              setKind("people");
            }}
          />
          <span>
            <label htmlFor={`${id}-people`} className="whisper-setup__kind-label">
              {t("voice.whisper.people")}
            </label>
            <span id={`${id}-people-hint`} className="settings-hint">
              {t("voice.whisper.peopleHint")}
            </span>
          </span>
        </div>
        {receivers.kind === "people" && (
          <div className="whisper-setup__options">
            {peopleChoices.length === 0 ? (
              <p className="settings-hint">
                {server.connected ? t("voice.whisper.noPeople") : t("voice.whisper.noServer")}
              </p>
            ) : (
              peopleChoices.map((person) => (
                <label key={person} className="whisper-setup__check">
                  <input
                    type="checkbox"
                    checked={names.includes(person)}
                    onChange={(event) => {
                      setReceivers({
                        kind: "people",
                        names: event.target.checked
                          ? [...names, person]
                          : names.filter((n) => n !== person),
                      });
                    }}
                  />
                  {person}
                </label>
              ))
            )}
          </div>
        )}
        <div className="whisper-setup__kind">
          <input
            id={`${id}-channel`}
            type="radio"
            name={`${id}-kind`}
            checked={receivers.kind === "channel"}
            aria-describedby={`${id}-channel-hint`}
            onChange={() => {
              setKind("channel");
            }}
          />
          <span>
            <label htmlFor={`${id}-channel`} className="whisper-setup__kind-label">
              {t("voice.whisper.channel")}
            </label>
            <span id={`${id}-channel-hint`} className="settings-hint">
              {t("voice.whisper.channelHint")}
            </span>
          </span>
        </div>
        {receivers.kind === "channel" && (
          <div className="whisper-setup__options">
            {channels.length === 0 && !channelKnown && pathKey === "" ? (
              <p className="settings-hint">{t("voice.whisper.noServer")}</p>
            ) : (
              <>
                <label className="visually-hidden" htmlFor={`${id}-channel`}>
                  {t("voice.whisper.channelPick")}
                </label>
                <select
                  id={`${id}-channel`}
                  className="settings-input"
                  value={pathKey}
                  onChange={(event) => {
                    const path = channels.find((p) => p.join("/") === event.target.value) ?? [];
                    setReceivers({ ...receivers, path });
                  }}
                >
                  {!channelKnown && pathKey !== "" && (
                    <option value={pathKey}>
                      {t("voice.whisper.missingChannel", { channel: receivers.path.join(" / ") })}
                    </option>
                  )}
                  {channels.map((path) => (
                    <option key={path.join("/")} value={path.join("/")}>
                      {path.join(" / ")}
                    </option>
                  ))}
                </select>
              </>
            )}
            <label className="whisper-setup__check">
              <input
                type="checkbox"
                checked={receivers.children}
                onChange={(event) => {
                  setReceivers({ ...receivers, children: event.target.checked });
                }}
              />
              {t("voice.whisper.children")}
            </label>
            <label className="whisper-setup__check">
              <input
                type="checkbox"
                checked={receivers.links}
                onChange={(event) => {
                  setReceivers({ ...receivers, links: event.target.checked });
                }}
              />
              {t("voice.whisper.links")}
            </label>
            <label className="whisper-setup__field" htmlFor={`${id}-group`}>
              {t("voice.whisper.group")}
            </label>
            <input
              id={`${id}-group`}
              className="settings-input"
              value={receivers.group ?? ""}
              aria-describedby={`${id}-group-hint`}
              onChange={(event) => {
                setReceivers({ ...receivers, group: event.target.value || null });
              }}
            />
            <p id={`${id}-group-hint`} className="settings-hint">
              {t("voice.whisper.groupHint")}
            </p>
          </div>
        )}
      </fieldset>
      <HotkeyRow
        targetId={target.id}
        hotkey={hotkey}
        unavailable={keysUnavailable !== null}
        failed={failedKey && hotkey === target.hotkey}
        onChange={setHotkey}
      />
      <div className="whisper-setup__actions">
        <button
          type="button"
          className="settings-button"
          onClick={() => {
            onDelete(target);
          }}
        >
          {t("voice.whisper.delete")}
        </button>
        <button type="submit" className="settings-button settings-button--primary">
          {t("voice.whisper.save")}
        </button>
      </div>
    </form>
  );
}

/** "Hold to talk: [F9] Change"; explains why keys cannot work in this session. */
function HotkeyRow({
  targetId,
  hotkey,
  unavailable,
  failed,
  onChange,
}: {
  targetId: string;
  hotkey: string | null;
  unavailable: boolean;
  failed: boolean;
  onChange: (hotkey: string | null) => void;
}) {
  const { t } = useTranslation("settings");
  const [listening, setListening] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const capture = async (combo: string) => {
    setListening(false);
    try {
      onChange(await getApi().whisperKeyCheck(targetId, combo));
      setProblem(null);
    } catch (error: unknown) {
      setProblem(
        toEngineError(error).code === "invalidInput"
          ? t("voice.whisper.keyTaken")
          : errorText(t, error),
      );
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!listening) return;
    // While listening every key belongs to the capture; Escape cancels it.
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setListening(false);
      return;
    }
    const combo = keyCombo(event);
    if (combo !== null) void capture(combo);
  };

  return (
    <div className="whisper-setup__hotkey">
      <p className="settings-value">
        {t("voice.whisper.hotkey")}{" "}
        <kbd className="settings-kbd">{hotkey ?? t("voice.whisper.keyNotSet")}</kbd>
      </p>
      {!unavailable && (
        <div className="settings-actions">
          <button
            type="button"
            className="settings-button"
            aria-pressed={listening}
            onClick={() => {
              setProblem(null);
              setListening(!listening);
            }}
            onKeyDown={onKeyDown}
            onBlur={() => {
              setListening(false);
            }}
          >
            {listening
              ? t("voice.whisper.keyListening")
              : hotkey === null
                ? t("voice.whisper.keySet")
                : t("voice.whisper.keyChange")}
          </button>
          {hotkey !== null && (
            <button
              type="button"
              className="settings-button"
              onClick={() => {
                onChange(null);
              }}
            >
              {t("voice.whisper.keyRemove")}
            </button>
          )}
        </div>
      )}
      {unavailable && <p className="settings-warning">{t("voice.whisper.keysWayland")}</p>}
      {failed && !unavailable && <p className="settings-warning">{t("voice.whisper.keyFailed")}</p>}
      <p role="alert" className="settings-error">
        {problem}
      </p>
    </div>
  );
}
