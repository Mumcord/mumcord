import { useEffect, useId, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { describeError } from "./errors";
import { pickImagePath } from "./files";
import { Modal } from "./Modal";
import { htmlToText, textToHtml } from "./text";

type AvatarChange = { kind: "keep" } | { kind: "remove" } | { kind: "set"; path: string };

interface ProfileDialogProps {
  session: number;
  hasAvatar: boolean;
  registered: boolean;
  onClose: () => void;
}

export function ProfileDialog({ session, hasAvatar, registered, onClose }: ProfileDialogProps) {
  const { t } = useTranslation(["userpanel", "common"]);
  const commentId = useId();
  const errorId = useId();
  const [comment, setComment] = useState("");
  const [original, setOriginal] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [currentAvatar, setCurrentAvatar] = useState<string | null>(null);
  const [avatar, setAvatar] = useState<AvatarChange>({ kind: "keep" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    getApi()
      .personProfile(session)
      .then((profile) => {
        if (!active) return;
        const text = htmlToText(profile.comment ?? "");
        setComment(text);
        setOriginal(text);
        setCurrentAvatar(profile.avatar);
      })
      .catch((e: unknown) => {
        if (active) setError(describeError(t, e));
      })
      .finally(() => {
        if (active) setLoaded(true);
      });
    return () => {
      active = false;
    };
  }, [session, t]);

  const choose = async () => {
    try {
      const path = await pickImagePath(t("profile.chooseTitle"));
      if (path !== null) setAvatar({ kind: "set", path });
    } catch (e) {
      setError(describeError(t, e));
    }
  };

  const save = async (event: SyntheticEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    const api = getApi();
    try {
      if (avatar.kind === "set") await api.setOwnAvatar(avatar.path);
      else if (avatar.kind === "remove") await api.setOwnAvatar(null);
      if (comment !== original) await api.setOwnComment(textToHtml(comment));
      onClose();
    } catch (e) {
      setError(describeError(t, e));
      setBusy(false);
    }
  };

  const fileName = avatar.kind === "set" ? (avatar.path.split(/[\\/]/).pop() ?? avatar.path) : null;
  const showsAvatar = avatar.kind === "keep" && hasAvatar;

  return (
    <Modal title={t("profile.title")} onClose={onClose}>
      <form onSubmit={(e) => void save(e)} className="userpanel-form">
        <div className="userpanel-form__row">
          {showsAvatar && currentAvatar ? (
            <img className="userpanel-avatar userpanel-avatar--large" src={currentAvatar} alt="" />
          ) : null}
          <div className="userpanel-form__buttons">
            <button type="button" className="button" onClick={() => void choose()}>
              {t("profile.chooseAvatar")}
            </button>
            {(hasAvatar && avatar.kind === "keep") || avatar.kind === "set" ? (
              <button
                type="button"
                className="button"
                onClick={() => {
                  setAvatar({ kind: "remove" });
                }}
              >
                {t("profile.removeAvatar")}
              </button>
            ) : null}
          </div>
        </div>
        <p className="userpanel-hint" role="status">
          {fileName
            ? t("profile.avatarChosen", { name: fileName })
            : avatar.kind === "remove"
              ? t("profile.avatarRemoved")
              : t("profile.avatarHint")}
        </p>
        {registered ? null : <p className="userpanel-hint">{t("profile.avatarUnregistered")}</p>}
        <label htmlFor={commentId}>{t("profile.comment")}</label>
        <textarea
          id={commentId}
          rows={5}
          value={comment}
          disabled={!loaded}
          aria-describedby={error ? errorId : undefined}
          onChange={(e) => {
            setComment(e.target.value);
          }}
        />
        {error ? (
          <p id={errorId} className="userpanel-error" role="alert">
            {error}
          </p>
        ) : null}
        <div className="userpanel-form__actions">
          <button type="button" className="button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button type="submit" className="button button--primary" disabled={busy || !loaded}>
            {t("common:save")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
