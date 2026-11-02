import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type KeyChangeReview, type RepositoryView } from "../../../api";
import { ModalDialog } from "../../settings/components/ModalDialog";
import { repositoryProblem } from "./describeError";

interface KeyChangeDialogProps {
  repository: RepositoryView;
  onDone: () => void;
  onRemove: () => void;
  onCancel: () => void;
}

/** FR-6.75: a repository that signs with another key stays paused until the user decides. */
export function KeyChangeDialog({ repository, onDone, onRemove, onCancel }: KeyChangeDialogProps) {
  const { t } = useTranslation(["pluginRepos", "common"]);
  const [review, setReview] = useState<KeyChangeReview | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let current = true;
    getApi()
      .pluginRepoReviewKey(repository.id)
      .then((result) => {
        if (current) setReview(result);
      })
      .catch((error: unknown) => {
        if (current) setProblem(repositoryProblem(error));
      });
    return () => {
      current = false;
    };
  }, [repository.id]);

  const trust = async () => {
    setBusy(true);
    try {
      await getApi().pluginRepoAcceptKey(repository.id);
      onDone();
    } catch (error: unknown) {
      setProblem(repositoryProblem(error));
      setBusy(false);
    }
  };

  return (
    <ModalDialog title={t("keyChange.title", { name: repository.name })} onClose={onCancel}>
      <p>{t("keyChange.message")}</p>
      {review !== null && (
        <dl className="plugin-repos__facts">
          <dt>{t("keyChange.oldKey")}</dt>
          <dd className="plugin-repos__fingerprint">{review.oldFingerprint}</dd>
          <dt>{t("keyChange.newKey")}</dt>
          <dd className="plugin-repos__fingerprint">{review.newFingerprint}</dd>
        </dl>
      )}
      {problem !== null && (
        <p role="alert" className="plugin-repos__problem">
          {problem}
        </p>
      )}
      <div className="settings-dialog__actions">
        <button type="button" className="settings-button" onClick={onCancel}>
          {t("keyChange.keepPaused")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--danger"
          onClick={onRemove}
        >
          {t("remove.confirm")}
        </button>
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={busy || review === null}
          onClick={() => void trust()}
        >
          {t("keyChange.trust")}
        </button>
      </div>
    </ModalDialog>
  );
}
