import { useId, useRef, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type MumbleLink } from "../../api";
import { DEFAULT_PORT, isMumbleLink, splitAddress } from "./format";
import { addServer, openDialog, requestConnect, toApiError, useServers } from "./store";

/**
 * Shown instead of the status bar while no server is saved: one address field (or a pasted
 * `mumble://` link) adds the server with sensible defaults and connects right away.
 */
export function EmptyState() {
  const { t } = useTranslation("servers");
  const { identityName } = useServers();
  const id = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [hint, setHint] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async (link: MumbleLink | null, address: string, port: number) => {
    const server = await addServer({
      label: address,
      address,
      port,
      username: link?.username ?? identityName,
      password: link?.password ? { kind: "set", value: link.password } : { kind: "keep" },
    });
    requestConnect(server.id, link && link.channelPath.length > 0 ? link.channelPath : null);
  };

  const submit = async (text: string) => {
    if (text.trim() === "") {
      setError(t("empty.errors.required"));
      inputRef.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (isMumbleLink(text)) {
        const link = await getApi().parseMumbleLink(text.trim());
        await save(link, link.address, link.port);
      } else {
        const split = splitAddress(text);
        await save(null, split.address, split.port ?? DEFAULT_PORT);
      }
    } catch (failure) {
      const code = toApiError(failure).code;
      setError(
        code === "invalidInput"
          ? isMumbleLink(text)
            ? t("dialog.errors.link")
            : t("dialog.errors.invalid")
          : t("errors.generic"),
      );
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  const pasteLink = async () => {
    setHint(null);
    try {
      const text = await navigator.clipboard.readText();
      setValue(text);
      if (isMumbleLink(text)) {
        await submit(text);
        return;
      }
      setHint(t("empty.noLinkInClipboard"));
    } catch {
      // Clipboard access can be refused by the webview; typing Ctrl+V still works.
      setHint(t("empty.pasteManually"));
    }
    inputRef.current?.focus();
  };

  const describedBy = [`${id}-hint`, error ? `${id}-error` : null, hint ? `${id}-paste` : null]
    .filter(Boolean)
    .join(" ");

  return (
    <section className="servers-empty" aria-labelledby={`${id}-title`}>
      <h2 id={`${id}-title`}>{t("empty.title")}</h2>
      <p>{t("empty.description")}</p>
      <form
        className="servers-empty__form"
        noValidate
        onSubmit={(event: SubmitEvent) => {
          event.preventDefault();
          void submit(value);
        }}
      >
        <label htmlFor={`${id}-address`}>{t("empty.addressLabel")}</label>
        <div className="servers-empty__row">
          <input
            ref={inputRef}
            id={`${id}-address`}
            value={value}
            autoComplete="off"
            spellCheck={false}
            placeholder="mumble.example.org"
            aria-invalid={error !== null}
            aria-describedby={describedBy}
            onChange={(event) => {
              setValue(event.target.value);
              setError(null);
            }}
          />
          <button type="submit" className="button button--primary" disabled={busy}>
            {t("empty.connect")}
          </button>
        </div>
        <p id={`${id}-hint`} className="servers-form__hint">
          {t("empty.addressHint")}
        </p>
        {error && (
          <p id={`${id}-error`} className="servers-form__error" role="alert">
            {error}
          </p>
        )}
        {hint && (
          <p id={`${id}-paste`} className="servers-form__hint">
            {hint}
          </p>
        )}
      </form>
      <div className="servers-empty__actions">
        <button type="button" className="button" disabled={busy} onClick={() => void pasteLink()}>
          {t("empty.pasteLink")}
        </button>
        <button
          type="button"
          className="button"
          onClick={() => {
            openDialog({ kind: "add", link: null });
          }}
        >
          {t("empty.moreOptions")}
        </button>
      </div>
    </section>
  );
}
