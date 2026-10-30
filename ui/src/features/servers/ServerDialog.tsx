import { useId, useState, type KeyboardEvent, type SubmitEvent } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import {
  getApi,
  type MumbleLink,
  type PasswordUpdate,
  type SavedServer,
  type TestConnection,
} from "../../api";
import { openSettingsSection } from "../settings/settingsOpener";
import { ChannelPasswordList } from "./ChannelPasswordList";
import { Dialog } from "./Dialog";
import { DEFAULT_PORT, isMumbleLink, splitAddress } from "./format";
import { IdentitySelect } from "./IdentitySelect";
import { serverLimitText } from "./errors";
import { ServerThemeSelect } from "./ServerThemeSelect";
import {
  addServer,
  getServerLimit,
  refreshIdentities,
  requestConnect,
  saveAlwaysConnect,
  saveServerTheme,
  toApiError,
  updateServer,
  useServers,
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
  /** Identity to connect with; `null` uses the default (issue #148). */
  identityId: number | null;
}

type Tab = "connection" | "identity" | "passwords" | "behaviour";

const TABS: readonly Tab[] = ["connection", "identity", "passwords", "behaviour"];

/** Validated fields; every one has a message key for its error. */
type Field = "address" | "port" | "username" | "label";

const TAB_OF: Record<Field, Tab> = {
  address: "connection",
  port: "connection",
  username: "connection",
  label: "connection",
};

/** The tab the last edit used, kept for the app session; a new dialog starts on Connection. */
const LAST_TAB_KEY = "mumcord:server-dialog-tab";

function lastEditTab(): Tab {
  const saved = sessionStorage.getItem(LAST_TAB_KEY);
  return TABS.includes(saved as Tab) ? (saved as Tab) : "connection";
}

/** Label length the server bar can show; longer names are refused (FR-3.36). */
const MAX_LABEL = 64;

type TestState =
  | { status: "idle" }
  | { status: "busy" }
  | { status: "done"; result: TestConnection; address: string }
  | { status: "error"; message: string; address: string };

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
      identityId: server.identityId,
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
    identityId: null,
  };
}

/** The first rule `fields` breaks, as a message key; `null` when everything is fine. */
function check(field: Field, fields: Fields): string | null {
  switch (field) {
    case "address": {
      const address = fields.address.trim();
      if (address === "" || isMumbleLink(fields.address)) return "dialog.errors.address";
      if (address.includes(" ")) return "dialog.errors.addressInvalid";
      return null;
    }
    case "port": {
      if (fields.port.trim() === "") return "dialog.errors.portEmpty";
      const port = Number(fields.port);
      return Number.isInteger(port) && port >= 1 && port <= 65535 ? null : "dialog.errors.port";
    }
    case "username":
      return fields.username.trim() === "" ? "dialog.errors.username" : null;
    case "label":
      return fields.label.trim().length > MAX_LABEL ? "dialog.errors.labelLong" : null;
  }
}

const FIELDS: readonly Field[] = ["address", "port", "username", "label"];

/**
 * Add/edit form for a saved server (FR-1.7, issue #151): one component for both, grouped in four
 * tabs, with inline validation and a test connection that uses the values as typed. The saved
 * password is write-only.
 */
export function ServerDialog({ server, link, identityName, onClose }: ServerDialogProps) {
  const { t } = useTranslation(["servers", "common"]);
  const identities = useServers().identities;
  const connected = useServers().connections.has(server?.id ?? -1);
  const id = useId();
  const [fields, setFields] = useState(() => initialFields(server, link, identityName));
  const [tab, setTab] = useState<Tab>(server ? lastEditTab() : "connection");
  const [channelPath, setChannelPath] = useState<string[] | null>(
    link && link.channelPath.length > 0 ? link.channelPath : null,
  );
  const [fromLink, setFromLink] = useState(link !== null);
  const [errors, setErrors] = useState<Partial<Record<Field, string | undefined>>>({});
  const [linkError, setLinkError] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [test, setTest] = useState<TestState>({ status: "idle" });
  const [removedTokens, setRemovedTokens] = useState<number[]>([]);
  const [renamedTokens, setRenamedTokens] = useState<ReadonlyMap<number, string>>(new Map());
  const connectAfter = link !== null;

  const set = (patch: Partial<Fields>) => {
    setFields((current) => ({ ...current, ...patch }));
  };

  const showTab = (next: Tab) => {
    setTab(next);
    if (server) sessionStorage.setItem(LAST_TAB_KEY, next);
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

  const password = (): PasswordUpdate => {
    if (fields.clearPassword) return { kind: "clear" };
    if (fields.password !== "") return { kind: "set", value: fields.password };
    return { kind: "keep" };
  };

  /** Checks `fieldsToCheck` and stores their messages; true when all of them pass. */
  const validate = (fieldsToCheck: readonly Field[]): boolean => {
    const found: Partial<Record<Field, string | undefined>> = {};
    for (const field of fieldsToCheck) {
      const problem = check(field, fields);
      if (problem !== null) found[field] = problem;
    }
    setErrors(found);
    return Object.keys(found).length === 0;
  };

  /** Validates one field after it lost focus and keeps its message in step (FR-3.35). */
  const setFieldError = (field: Field) => {
    const problem = check(field, fields);
    setErrors((current) => ({ ...current, [field]: problem ?? undefined }));
  };

  const focusField = (field: Field) => {
    document.getElementById(`${id}-${field}`)?.focus();
  };

  const onSubmit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!validate(FIELDS)) {
      // Jump to the tab of the first invalid field, then to the field itself (FR-3.37).
      const first = FIELDS.find((field) => check(field, fields) !== null);
      if (first !== undefined) {
        const target = TAB_OF[first];
        showTab(target);
        // Only a different tab needs a frame: its panel is not rendered yet.
        if (target === tab) focusField(first);
        else
          requestAnimationFrame(() => {
            focusField(first);
          });
      }
      return;
    }
    const split = splitAddress(fields.address);
    const address = split.address;
    const port = split.port ?? Number(fields.port);
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
      if (fields.identityId !== (server?.identityId ?? null)) {
        await getApi().serverSetIdentity(saved.id, fields.identityId);
        await refreshIdentities();
      }
      if (fields.alwaysConnect !== (server?.alwaysConnectAtStart ?? false)) {
        await saveAlwaysConnect(saved.id, fields.alwaysConnect);
      }
      if (fields.themeId !== (server?.themeId ?? null)) {
        await saveServerTheme(saved.id, fields.themeId);
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

  const testConnection = async () => {
    if (!validate(["address", "port", "username"])) {
      showTab("connection");
      return;
    }
    const split = splitAddress(fields.address);
    const address = split.address;
    const port = split.port ?? Number(fields.port);
    setTest({ status: "busy" });
    try {
      const result = await getApi().serverTestConnection({
        address,
        port,
        username: fields.username.trim(),
        password: password(),
        serverId: server?.id ?? null,
        identityId: fields.identityId,
      });
      setTest({ status: "done", result, address });
    } catch (error) {
      const { code } = toApiError(error);
      console.error("Test connection failed", code);
      setTest({ status: "error", message: testErrorText(t, code), address });
    }
  };

  const errorId = (field: Field) => `${id}-${field}-error`;
  const fieldProps = (field: Field) => ({
    "aria-invalid": errors[field] !== undefined,
    "aria-describedby": errors[field] === undefined ? undefined : errorId(field),
  });
  const fieldError = (field: Field) => {
    const key = errors[field];
    if (key === undefined) return null;
    return (
      <p id={errorId(field)} className="servers-form__error" role="alert">
        {t(key)}
      </p>
    );
  };

  const tabsWithErrors = new Set(
    FIELDS.filter((field) => errors[field] !== undefined).map((field) => TAB_OF[field]),
  );

  const onTabKeyDown = (event: KeyboardEvent) => {
    const index = TABS.indexOf(tab);
    switch (event.key) {
      case "ArrowRight":
        showTab(TABS[(index + 1) % TABS.length] ?? "connection");
        break;
      case "ArrowLeft":
        showTab(TABS[(index - 1 + TABS.length) % TABS.length] ?? "connection");
        break;
      case "Home":
        showTab(TABS[0] ?? "connection");
        break;
      case "End":
        showTab(TABS[TABS.length - 1] ?? "connection");
        break;
      default:
        return;
    }
    event.preventDefault();
  };

  const tabLabel = (name: Tab) => t(`dialog.tabs.${name}`);

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
      <div className="servers-tabs" role="tablist" aria-label={t("dialog.tabsLabel")}>
        {TABS.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            id={`${id}-tab-${name}`}
            aria-selected={tab === name}
            aria-controls={`${id}-panel-${name}`}
            tabIndex={tab === name ? 0 : -1}
            className="servers-tabs__tab"
            onKeyDown={onTabKeyDown}
            onClick={() => {
              showTab(name);
            }}
          >
            {tabLabel(name)}
            {tabsWithErrors.has(name) && (
              <>
                <span className="servers-tabs__dot" aria-hidden="true">
                  !
                </span>
                <span className="visually-hidden">
                  {t("dialog.tabs.needsAttention", { tab: tabLabel(name) })}
                </span>
              </>
            )}
          </button>
        ))}
      </div>
      <form className="servers-form" onSubmit={(event) => void onSubmit(event)} noValidate>
        {tab === "connection" && (
          <div
            role="tabpanel"
            id={`${id}-panel-connection`}
            aria-labelledby={`${id}-tab-connection`}
            className="servers-form__panel"
          >
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
                onBlur={() => {
                  setFieldError("address");
                }}
                aria-invalid={errors.address !== undefined}
                aria-describedby={`${id}-address-hint${errors.address === undefined ? "" : ` ${errorId("address")}`}`}
              />
              <p id={`${id}-address-hint`} className="servers-form__hint">
                {fromLink ? t("dialog.filledFromLink") : t("dialog.addressHint")}
              </p>
              {linkError && (
                <p className="servers-form__error" role="alert">
                  {t("dialog.errors.link")}
                </p>
              )}
              {fieldError("address")}
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
                onBlur={() => {
                  setFieldError("port");
                }}
                {...fieldProps("port")}
              />
              {fieldError("port")}
            </div>
            <div className="servers-form__field">
              <label htmlFor={`${id}-label`}>{t("dialog.label")}</label>
              <input
                id={`${id}-label`}
                value={fields.label}
                placeholder={fields.address}
                maxLength={MAX_LABEL * 2}
                onChange={(event) => {
                  set({ label: event.target.value });
                }}
                onBlur={() => {
                  setFieldError("label");
                }}
                {...fieldProps("label")}
              />
              {fieldError("label")}
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
                onBlur={() => {
                  setFieldError("username");
                }}
                {...fieldProps("username")}
              />
              {fieldError("username")}
            </div>
            <div className="servers-form__field">
              <button
                type="button"
                className="button"
                disabled={test.status === "busy"}
                onClick={() => void testConnection()}
              >
                {test.status === "busy" ? t("dialog.test.busy") : t("dialog.test.button")}
              </button>
              <TestResult state={test} />
            </div>
          </div>
        )}
        {tab === "identity" && (
          <div
            role="tabpanel"
            id={`${id}-panel-identity`}
            aria-labelledby={`${id}-tab-identity`}
            className="servers-form__panel"
          >
            <IdentitySelect
              label={t("identity.field")}
              value={fields.identityId}
              identities={identities}
              onChange={(identityId) => {
                set({ identityId });
              }}
              hint={t("identity.intro")}
              hintId={`${id}-identity-hint`}
            />
            {server && connected && fields.identityId !== server.identityId && (
              <p className="servers-form__hint">{t("identity.nextConnect")}</p>
            )}
            <button
              type="button"
              className="servers-form__link"
              onClick={() => {
                onClose();
                openSettingsSection("identity");
              }}
            >
              {t("identity.manage")}
            </button>
          </div>
        )}
        {tab === "passwords" && (
          <div
            role="tabpanel"
            id={`${id}-panel-passwords`}
            aria-labelledby={`${id}-tab-passwords`}
            className="servers-form__panel"
          >
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
          </div>
        )}
        {tab === "behaviour" && (
          <div
            role="tabpanel"
            id={`${id}-panel-behaviour`}
            aria-labelledby={`${id}-tab-behaviour`}
            className="servers-form__panel"
          >
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

/** Plain-language text for a refused test connection. */
function testErrorText(t: TFunction<"servers">, code: string): string {
  switch (code) {
    case "wrongPassword":
      return t("dialog.test.wrongPassword");
    case "nameInUse":
      return t("dialog.test.nameInUse");
    case "serverUnreachable":
    case "localNetworkBlocked":
      return t("dialog.test.unreachable");
    case "invalidInput":
      return t("dialog.test.invalid");
    default:
      return t("dialog.test.failed");
  }
}

const CERTIFICATE_TEXT: Record<TestConnection["certificate"], string> = {
  trusted: "dialog.test.certTrusted",
  unknown: "dialog.test.certUnknown",
  changed: "dialog.test.certChanged",
};

/** Result line of the test connection (FR-3.41, FR-3.42). */
function TestResult({ state }: { state: TestState }) {
  const { t } = useTranslation("servers");
  if (state.status === "idle") return null;
  if (state.status === "busy") {
    return (
      <p className="servers-form__hint" role="status">
        {t("dialog.test.running")}
      </p>
    );
  }
  if (state.status === "error") {
    return (
      <p className="servers-form__error" role="status">
        {state.address}: {state.message}
      </p>
    );
  }
  const { result, address } = state;
  const tone =
    result.certificate === "changed"
      ? "servers-form__error"
      : result.certificate === "unknown"
        ? "servers-form__hint"
        : "servers-form__success";
  return (
    <div role="status" className="servers-form__test">
      <p className="servers-form__success">
        {t("dialog.test.result", {
          address,
          ms: result.pingMs,
          version: result.version,
        })}
      </p>
      <p className={tone}>{t(CERTIFICATE_TEXT[result.certificate])}</p>
    </div>
  );
}
