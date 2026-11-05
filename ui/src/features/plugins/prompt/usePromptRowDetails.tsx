import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

/** One website a plugin declares; `purpose` is the plugin's own text (plain, untrusted). */
export interface PromptOrigin {
  origin: string;
  purpose: string | null;
}

/** Places a sealed frame can be shown, in the order the mockup lists them. */
const FRAME_PLACES = [
  ["ui.stage", "stage"],
  ["ui.side-panel", "side-panel"],
  ["ui.dialog", "dialog"],
  ["ui.panel", "panel"],
] as const;

/**
 * What the install prompt adds to two permission rows (surfaces mockup 01): who wrote the
 * plugin's own code and where its sealed frame shows, and every website with its purpose and the
 * privacy line (FR-9.29).
 */
export function usePromptRowDetails({
  pluginName,
  repository,
  permissions,
  origins,
}: {
  pluginName: string;
  /** Third-party repository the code comes from; official plugins keep the plain explanation. */
  repository: string | null;
  /** Every permission the plugin asks for (required and optional). */
  permissions: readonly string[];
  origins: readonly PromptOrigin[];
}): Partial<Record<string, ReactNode>> {
  const { t } = useTranslation("pluginGate");
  const { t: tOrigins } = useTranslation("pluginOrigins");
  const details: Partial<Record<string, ReactNode>> = {};

  const places = FRAME_PLACES.filter(([id]) => permissions.includes(id)).map(([, key]) =>
    t(`places.${key}`),
  );
  if (repository !== null && permissions.includes("ui.webview") && places.length > 0) {
    details["ui.webview"] = (
      <>
        <span className="perm-reason">{t("runsCode", { author: repository })}</span>
        <span className="perm-detail">{t("sealedFrame", { places: places.join(", ") })}</span>
      </>
    );
  }

  if (permissions.includes("net.origins")) {
    const names = permissions.includes("events.presence") || permissions.includes("peers.messages");
    details["net.origins"] = (
      <>
        <span className="perm-reason">{tOrigins("intro", { plugin: pluginName })}</span>
        <span className="perm-detail">
          {tOrigins("privacy", { plugin: pluginName })}
          {names && ` ${tOrigins("includesNames")}`}
        </span>
        {origins.length > 0 && (
          <ul className="plugin-prompt-origins" aria-label={tOrigins("listLabel")}>
            {origins.map((o) => (
              <li key={o.origin} className="plugin-prompt-origin">
                <code>{o.origin}</code>
                {o.purpose !== null && o.purpose !== "" && <span>{o.purpose}</span>}
              </li>
            ))}
          </ul>
        )}
      </>
    );
  }
  return details;
}
