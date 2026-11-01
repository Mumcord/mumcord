import { useCallback, useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type RepositoryView } from "../../../api";
import { ConfirmDialog } from "../../settings/components/ConfirmDialog";
import { Switch } from "../../settings/components/Switch";
import { AddRepositoryDialog } from "./AddRepositoryDialog";
import { consumePluginRequest, usePendingPluginRequest } from "../pluginRequestStore";
import { repositoryProblem } from "./describeError";
import { KeyChangeDialog } from "./KeyChangeDialog";
import "./repos.css";

/** The most repositories a user can add (FR-6.74). */
export const MAX_REPOSITORIES = 10;

export interface MirrorSetting {
  url: string | null;
  onChange: (url: string | null) => void;
}

/** Settings → Plugins → Repositories (FR-6.74 – FR-6.76, FR-6.57). */
export function RepositoriesTab({ mirror }: { mirror?: MirrorSetting | undefined }) {
  const { t, i18n } = useTranslation("pluginRepos");
  const [repositories, setRepositories] = useState<RepositoryView[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<RepositoryView | null>(null);
  const [reviewing, setReviewing] = useState<RepositoryView | null>(null);
  const request = usePendingPluginRequest("addRepository");

  const load = useCallback(async () => {
    try {
      setRepositories(await getApi().pluginReposList());
    } catch (error: unknown) {
      console.error("Loading the plugin repositories failed", error);
      setProblem(t("loadFailed"));
    }
  }, [t]);

  useEffect(() => {
    let current = true;
    const refresh = () => {
      getApi()
        .pluginReposList()
        .then((list) => {
          if (current) setRepositories(list);
        })
        .catch((error: unknown) => {
          console.error("Loading the plugin repositories failed", error);
          if (current) setProblem(t("loadFailed"));
        });
    };
    refresh();
    const off = getApi().on("pluginsChanged", refresh);
    return () => {
      current = false;
      off();
    };
  }, [t]);

  const change = async (action: () => Promise<unknown>) => {
    setProblem(null);
    try {
      await action();
    } catch (error: unknown) {
      setProblem(repositoryProblem(error));
    }
    await load();
  };

  const full = (repositories?.length ?? 0) >= MAX_REPOSITORIES;
  const showAdd = adding || request !== null;

  const closeAdd = () => {
    setAdding(false);
    consumePluginRequest("addRepository");
  };

  return (
    <div className="plugin-repos">
      <p className="plugin-repos__hint">{t("intro")}</p>
      <div className="plugin-repos__bar">
        <button
          type="button"
          className="settings-button settings-button--primary"
          disabled={full}
          onClick={() => {
            setAdding(true);
          }}
        >
          {t("add.button")}
        </button>
        <span className="plugin-repos__hint">
          {full
            ? t("limitReached", { max: MAX_REPOSITORIES })
            : t("limit", { max: MAX_REPOSITORIES })}
        </span>
      </div>
      {problem !== null && (
        <p role="alert" className="plugin-repos__problem">
          {problem}
        </p>
      )}
      {repositories !== null && repositories.length === 0 && (
        <p className="plugin-repos__hint">{t("empty")}</p>
      )}
      <ul className="plugin-repos__list" aria-label={t("listLabel")}>
        {repositories?.map((repository) => (
          <li key={repository.id} className="plugin-repos__item">
            <div className="plugin-repos__head">
              <strong>{repository.name}</strong>
              {repository.keyChanged && (
                <span className="plugin-repos__badge">{t("status.keyChanged")}</span>
              )}
            </div>
            <div className="plugin-repos__url">{repository.url}</div>
            <div className="plugin-repos__meta">
              {t("fingerprintLine", { fingerprint: repository.fingerprint })}
              {" · "}
              {repository.lastChecked === null
                ? t("neverChecked")
                : t("lastChecked", {
                    when: new Date(repository.lastChecked).toLocaleString(i18n.language),
                  })}
              {" · "}
              {t("installedCount", { number: repository.installedPlugins })}
            </div>
            <div className="plugin-repos__actions">
              {repository.keyChanged ? (
                <button
                  type="button"
                  className="settings-button"
                  onClick={() => {
                    setReviewing(repository);
                  }}
                >
                  {t("keyChange.review")}
                </button>
              ) : (
                <Switch
                  label={t("toggle", { name: repository.name })}
                  checked={repository.enabled}
                  onChange={(enabled) =>
                    void change(() => getApi().pluginRepoSetEnabled(repository.id, enabled))
                  }
                />
              )}
              <button
                type="button"
                className="settings-button settings-button--danger"
                aria-label={t("remove.label", { name: repository.name })}
                onClick={() => {
                  setRemoving(repository);
                }}
              >
                {t("remove.button")}
              </button>
            </div>
          </li>
        ))}
      </ul>
      {mirror !== undefined && <MirrorField mirror={mirror} />}
      {showAdd && (
        <AddRepositoryDialog
          initialUrl={request?.url ?? ""}
          initialKey={request?.key ?? null}
          onAdded={() => {
            closeAdd();
            void load();
          }}
          onCancel={closeAdd}
        />
      )}
      {removing !== null && (
        <ConfirmDialog
          title={t("remove.title", { name: removing.name })}
          message={t("remove.message")}
          confirmLabel={t("remove.confirm")}
          onCancel={() => {
            setRemoving(null);
          }}
          onConfirm={() => {
            const target = removing;
            setRemoving(null);
            void change(() => getApi().pluginRepoRemove(target.id));
          }}
        />
      )}
      {reviewing !== null && (
        <KeyChangeDialog
          repository={reviewing}
          onCancel={() => {
            setReviewing(null);
          }}
          onDone={() => {
            setReviewing(null);
            void load();
          }}
          onRemove={() => {
            const target = reviewing;
            setReviewing(null);
            void change(() => getApi().pluginRepoRemove(target.id));
          }}
        />
      )}
    </div>
  );
}

/** FR-6.57: an optional mirror for the official list, kept out of the way under "Advanced". */
function MirrorField({ mirror }: { mirror: MirrorSetting }) {
  const { t } = useTranslation(["pluginRepos", "common"]);
  const id = useId();
  const [text, setText] = useState(mirror.url ?? "");
  const trimmed = text.trim();
  return (
    <details className="plugin-repos__advanced">
      <summary>{t("mirror.title")}</summary>
      <label htmlFor={`${id}-mirror`}>{t("mirror.label")}</label>
      <input
        id={`${id}-mirror`}
        className="settings-input"
        type="url"
        autoComplete="off"
        spellCheck={false}
        placeholder="https://"
        aria-describedby={`${id}-mirror-hint`}
        value={text}
        onChange={(event) => {
          setText(event.target.value);
        }}
      />
      <p id={`${id}-mirror-hint`} className="plugin-repos__hint">
        {t("mirror.hint")}
      </p>
      <div className="settings-actions">
        <button
          type="button"
          className="settings-button"
          disabled={trimmed === (mirror.url ?? "")}
          onClick={() => {
            mirror.onChange(trimmed === "" ? null : trimmed);
          }}
        >
          {t("mirror.save")}
        </button>
        <button
          type="button"
          className="settings-button"
          disabled={mirror.url === null && trimmed === ""}
          onClick={() => {
            setText("");
            mirror.onChange(null);
          }}
        >
          {t("mirror.reset")}
        </button>
      </div>
    </details>
  );
}
