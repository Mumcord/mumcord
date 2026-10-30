import { useId, useState, type SubmitEvent } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type MumbleLink, type PasswordUpdate, type SavedServer } from "../../api";
import { ChannelPasswordList } from "./ChannelPasswordList";
import { Dialog } from "./Dialog";
import { DEFAULT_PORT, isMumbleLink, splitAddress } from "./format";
import { serverLimitText } from "./errors";
import { ServerThemeSelect } from "./ServerThemeSelect";
import {
  addServer,
  getServerLimit,
  requestConnect,
  saveAlwaysConnect,
  saveServerTheme,
  toApiError,
  updateServer,
} from "./store";

interface ServerDialogProps {
  /** Server being edited; `null` adds a new one. */
  server: SavedServer | null;
  /** Pre-filled from a `mumble://` link; the dialog then saves and connects (FR-1.11). */
  link: MumbleLink | null;
  identityName: string;
  onClose: () => void;
}

interface Fields {
  label: string;
  address: string;
  port: string;
  username: string;
  password: string;
  clearPassword: boolean;
  alwaysConnect: boolean;
  themeId: string | null;
}

function initialFields(
  server: SavedServer | null,
  link: MumbleLink | null,
  identityName: string,
): Fields {
  if (server) {
    return {
      label: server.label,
      address: server.address,
      port: String(server.port),
      username: server.username,
      password: "",
      clearPassword: false,
      alwaysConnect: server.alwaysConnectAtStart,
      themeId: server.themeId,
    };
  }
  return {
    label: link?.address ?? "",
    address: link?.address ?? "",
    port: String(link?.port ?? DEFAULT_PORT),
    username: link?.username ?? identityName,
    password: link?.password ?? "",
    clearPassword: false,
    alwaysConnect: false,
    themeId: null,
  };
}

type FieldError = "address" | "port" | "username";

/** Add/edit form for a saved server (FR-1.7). The saved password is write-only. */
export function ServerDialog({ server, link, identityName, onClose }: ServerDialogProps) {
  const { t } = useTranslation(["servers", "common"]);
  const id = useId();
  const [fields, setFields] = useState(() => initialFields(server, link, identityName));
  const [channelPath, setChannelPath] = useState<string[] | null>(
    link && link.channelPath.length > 0 ? link.channelPath : null,
  );
  const [fromLink, setFromLink] = useState(link !== null);
  const [invalid, setInvalid] = useState<FieldError | null>(null);
  const [linkError, setLinkError] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [removedTokens, setRemovedTokens] = useState<number[]>([]);
  const [renamedTokens, setRenamedTokens] = useState<ReadonlyMap<number, string>>(new Map());
  const connectAfter = link !== null;

  const set = (patch: Partial<Fields>) => {
    setFields((current) => ({ ...current, ...patch }));
  };

  const applyLink = async (url: string) => {
    try {
      const parsed = await getApi().parseMumbleLink(url.trim());
      setFields((current) => ({
        ...current,
        address: parsed.address,
        port: String(parsed.port),
        label: current.label || parsed.address,
        username: parsed.username ?? current.username,
        password: parsed.password ?? current.password,
      }));
      setChannelPath(parsed.channelPath.length > 0 ? parsed.channelPath : null);
      setFromLink(true);
      setLinkError(false);
    } catch (error) {
      toApiError(error);
      setLinkError(true);
    }
  };

  const onAddressChange = (value: string) => {
    set({ address: value });
    setLinkError(false);
    if (isMumbleLink(value)) void applyLink(value);
  };

  const validate = (): { port: number } | null => {
    const port = Number(fields.port);
    let field: FieldError | null = null;
    if (fields.address.trim() === "" || isMumbleLink(fields.address)) field = "address";
    else if (!Number.isInteger(port) || port < 1 || port > 65535) field = "port";
    else if (fields.username.trim() === "") field = "username";
    setInvalid(field);
    if (field === null) return { port };
    document.getElementById(`${id}-${field}`)?.focus();
    return null;
  };

  const password = (): PasswordUpdate => {
    if (fields.clearPassword) return { kind: "clear" };
    if (fields.password !== "") return { kind: "set", value: fields.password };
    return { kind: "keep" };
  };

  const onSubmit = async (event: SubmitEvent) => {
    event.preventDefault();
    const valid = validate();
    if (!valid) return;
    const split = splitAddress(fields.address);
    const address = split.address;
    const port = split.port ?? valid.port;
    const input = {
      label: fields.label.trim() || address,
      address,
      port,
      username: fields.username.trim(),
      password: password(),
    };
    setBusy(true);
    setFormError(null);
    try {
      const saved = server ? await updateServer(server.id, input) : await addServer(input);
      if (server && fields.alwaysConnect !== server.alwaysConnectAtStart) {
        await saveAlwaysConnect(server.id, fields.alwaysConnect);
      }
      if (server && fields.themeId !== server.themeId) {
        await saveServerTheme(server.id, fields.themeId);
      }
      // Renames go first: removing moves the later passwords up.
      if (server) {
        for (const [index, label] of renamedTokens) {
          if (!removedTokens.includes(index)) {
            await getApi().channelPasswordRename(server.id, index, label);
          }
        }
      }
      if (server && removedTokens.length > 0) {
        await getApi().channelPasswordsRemove(server.id, removedTokens);
      }
      onClose();
      if (connectAfter) requestConnect(saved.id, channelPath);
    } catch (error) {
      const apiError = toApiError(error);
      setFormError(
        apiError.code === "invalidInput"
          ? t("dialog.errors.invalid")
          : apiError.code === "secureStorageUnavailable"
            ? t("errors.secureStorage")
            : apiError.code === "serverLimitReached"
              ? serverLimitText(t, getServerLimit(), true)
              : t("errors.generic"),
      );
      setBusy(false);
    }
  };

  const errorId = (field: FieldError) => `${id}-${field}-error`;
  const fieldProps = (field: FieldError) => ({
    "aria-invalid": invalid === field,
    "aria-describedby": invalid === field ? errorId(field) : undefined,
  });

  const title = server
    ? t("dialog.editTitle")
    : connectAfter
      ? t("dialog.linkTitle")
      : t("dialog.addTitle");
  const submitLabel = server
    ? t("common:save")
    : connectAfter
      ? t("dialog.saveAndConnect")
      : t("dialog.add");

  return (
    <Dialog title={title} onClose={onClose}>
      <form className="servers-form" onSubmit={(event) => void onSubmit(event)} noValidate>
        <div className="servers-form__field">
          <label htmlFor={`${id}-address`}>{t("dialog.address")}</label>
          <input
            id={`${id}-address`}
            value={fields.address}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => {
              onAddressChange(event.target.value);
            }}
            aria-invalid={invalid === "address"}
            aria-describedby={`${id}-address-hint${invalid === "address" ? ` ${errorId("address")}` : ""}`}
          />
          <p id={`${id}-address-hint`} className="servers-form__hint">
            {fromLink ? t("dialog.filledFromLink") : t("dialog.addressHint")}
          </p>
          {linkError && (
            <p className="servers-form__error" role="alert">
              {t("dialog.errors.link")}
            </p>
          )}
          {invalid === "address" && (
            <p id={errorId("address")} className="servers-form__error">
              {t("dialog.errors.address")}
            </p>
          )}
        </div>
        <div className="servers-form__field">
          <label htmlFor={`${id}-port`}>{t("dialog.port")}</label>
          <input
            id={`${id}-port`}
            inputMode="numeric"
            value={fields.port}
            onChange={(event) => {
              set({ port: event.target.value });
            }}
            {...fieldProps("port")}
          />
          {invalid === "port" && (
            <p id={errorId("port")} className="servers-form__error">
              {t("dialog.errors.port")}
            </p>
          )}
        </div>
        <div className="servers-form__field">
          <label htmlFor={`${id}-label`}>{t("dialog.label")}</label>
          <input
            id={`${id}-label`}
            value={fields.label}
            placeholder={fields.address}
            onChange={(event) => {
              set({ label: event.target.value });
            }}
          />
        </div>
        <div className="servers-form__field">
          <label htmlFor={`${id}-username`}>{t("dialog.username")}</label>
          <input
            id={`${id}-username`}
            value={fields.username}
            autoComplete="off"
            onChange={(event) => {
              set({ username: event.target.value });
            }}
            {...fieldProps("username")}
          />
          {invalid === "username" && (
            <p id={errorId("username")} className="servers-form__error">
              {t("dialog.errors.username")}
            </p>
          )}
        </div>
        <div className="servers-form__field">
          <label htmlFor={`${id}-password`}>{t("dialog.password")}</label>
          <input
            id={`${id}-password`}
            type="password"
            value={fields.password}
            autoComplete="new-password"
            disabled={fields.clearPassword}
            aria-describedby={`${id}-password-hint`}
            onChange={(event) => {
              set({ password: event.target.value });
            }}
          />
          <p id={`${id}-password-hint`} className="servers-form__hint">
            {server?.hasPassword ? t("dialog.passwordKeepHint") : t("dialog.passwordHint")}
          </p>
          {server?.hasPassword && (
            <label className="servers-form__check">
              <input
                type="checkbox"
                checked={fields.clearPassword}
                onChange={(event) => {
                  set({ clearPassword: event.target.checked, password: "" });
                }}
              />
              {t("dialog.clearPassword")}
            </label>
          )}
        </div>
        {server && (
          <ChannelPasswordList
            serverId={server.id}
            removed={removedTokens}
            onRemove={(index) => {
              setRemovedTokens((current) => [...current, index]);
            }}
            renamed={renamedTokens}
            onRename={(index, label) => {
              setRenamedTokens((current) => new Map(current).set(index, label));
            }}
          />
        )}
        {server && (
          <div className="servers-form__field">
            <label className="servers-form__check">
              <input
                type="checkbox"
                checked={fields.alwaysConnect}
                aria-describedby={`${id}-always-hint`}
                onChange={(event) => {
                  set({ alwaysConnect: event.target.checked });
                }}
              />
              {t("dialog.alwaysConnect")}
            </label>
            <p id={`${id}-always-hint`} className="servers-form__hint">
              {t("dialog.alwaysConnectHint")}
            </p>
          </div>
        )}
        {server && (
          <div className="servers-form__field">
            <ServerThemeSelect
              label={t("theme.label")}
              value={fields.themeId}
              onChange={(themeId) => {
                set({ themeId });
              }}
            />
            <p className="servers-form__hint">{t("theme.hint")}</p>
          </div>
        )}
        {formError && (
          <p className="servers-form__error" role="alert">
            {formError}
          </p>
        )}
        <div className="servers-dialog__actions">
          <button type="button" className="button" onClick={onClose}>
            {t("common:cancel")}
          </button>
          <button type="submit" className="button button--primary" disabled={busy}>
            {submitLabel}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
