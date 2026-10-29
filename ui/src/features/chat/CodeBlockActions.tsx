import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { pickSavePath } from "../settings/fileDialogs";
import { codeOf, MAX_DOWNLOAD_BYTES } from "./codeBlock";
import { extensionFor } from "./highlight";

/** How long "Copied" / "Saved" stays visible. */
const FEEDBACK_MS = 2000;

interface Status {
  kind: "copied" | "saved" | "failed";
  text: string;
}

function Icon({ path }: { path: string }) {
  return (
    <svg viewBox="0 0 24 24" width={16} height={16} aria-hidden="true" focusable="false">
      <path
        d={path}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const COPY_ICON = "M9 9h11v11H9zM5 15H4V4h11v1";
const CHECK_ICON = "M5 12l5 5 9-10";
const DOWNLOAD_ICON = "M12 4v11m-5-5l5 5 5-5M5 20h14";

interface CodeBlockActionsProps {
  block: HTMLElement;
  /** Roving tab stop of the message. */
  tabIndex: number;
}

/** Copy and download buttons in the corner of one code block (issue #111). */
export function CodeBlockActions({ block, tabIndex }: CodeBlockActionsProps) {
  const { t } = useTranslation("chat");
  const [status, setStatus] = useState<Status | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
    },
    [],
  );

  function show(next: Status) {
    window.clearTimeout(timer.current);
    setStatus(next);
    if (next.kind !== "failed")
      timer.current = window.setTimeout(() => {
        setStatus(null);
      }, FEEDBACK_MS);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(codeOf(block));
      show({ kind: "copied", text: t("codeBlock.copied") });
    } catch (error) {
      console.error("Copying the code block failed", error);
      show({ kind: "failed", text: t("codeBlock.copyFailed") });
    }
  }

  async function download() {
    const code = codeOf(block);
    if (new TextEncoder().encode(code).length > MAX_DOWNLOAD_BYTES) {
      show({ kind: "failed", text: t("codeBlock.tooLarge") });
      return;
    }
    const extension = extensionFor(block.dataset["language"]);
    try {
      const path = await pickSavePath(
        `snippet.${extension}`,
        { name: t("codeBlock.fileType"), extensions: [extension] },
        t("codeBlock.download"),
      );
      if (path === null) return;
      await getApi().saveText(path, code);
      show({ kind: "saved", text: t("codeBlock.saved") });
    } catch (error) {
      console.error("Saving the code block failed", error);
      show({ kind: "failed", text: t("codeBlock.saveFailed") });
    }
  }

  return (
    <div className="chat-code__actions" data-active={status === null ? undefined : true}>
      <span
        role="status"
        className="chat-code__status"
        data-failed={status?.kind === "failed" ? true : undefined}
      >
        {status?.text}
      </span>
      <button
        type="button"
        className="chat-code__button"
        tabIndex={tabIndex}
        aria-label={t("codeBlock.copy")}
        data-tooltip={t("codeBlock.copy")}
        onClick={() => void copy()}
      >
        <Icon path={status?.kind === "copied" ? CHECK_ICON : COPY_ICON} />
      </button>
      <button
        type="button"
        className="chat-code__button"
        tabIndex={tabIndex}
        aria-label={t("codeBlock.download")}
        data-tooltip={t("codeBlock.download")}
        onClick={() => void download()}
      >
        <Icon path={DOWNLOAD_ICON} />
      </button>
    </div>
  );
}
