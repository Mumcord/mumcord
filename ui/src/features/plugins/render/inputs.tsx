/** Text-like plugin inputs (FR-6.41): text input with byte counter, password, file picker. */
import { open } from "@tauri-apps/plugin-dialog";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type PluginComponent } from "../../../api";
import { useRender } from "./context";
import { byteLength, clampBytes } from "./helpers";

type Of<T extends PluginComponent["type"]> = Extract<PluginComponent, { type: T }>;

/** Highest byte length of a text input (FR-6.41); the host refuses more. */
export const MAX_INPUT_BYTES = 64;

/** Sends the text on Enter or when the field is left, if it changed. */
export function PluginTextInput(props: Of<"textInput">) {
  const { label, value, placeholder, maxBytes, action, disabled } = props;
  const { send } = useRender();
  const { t } = useTranslation("pluginSlots");
  const id = useId();
  const max = Math.min(maxBytes, MAX_INPUT_BYTES);
  const [draft, setDraft] = useState<{ from: string; text: string }>({ from: value, text: value });
  const text = draft.from === value ? draft.text : value;
  const used = byteLength(text);
  const commit = () => {
    if (text !== value) send(action, { type: "text", value: text });
  };
  return (
    <div className="pr-field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type="text"
        className="pr-input"
        value={text}
        placeholder={placeholder}
        disabled={disabled}
        aria-describedby={`${id}-count`}
        spellCheck={props.purpose === "plain"}
        onChange={(e) => {
          setDraft({ from: value, text: clampBytes(e.currentTarget.value, max) });
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
        }}
        onBlur={commit}
      />
      <span id={`${id}-count`} className="pr-counter">
        {t("input.bytes", { used, max })}
      </span>
    </div>
  );
}

/**
 * Writes into the plugin's secret (the host stores it, the plugin never sees the text); the
 * field clears after saving.
 */
export function PluginPassword({ label, action, disabled }: Of<"password">) {
  const { send } = useRender();
  const { t } = useTranslation("pluginSlots");
  const id = useId();
  const [text, setText] = useState("");
  const [saved, setSaved] = useState(false);
  const save = () => {
    if (text === "") return;
    send(action, { type: "text", value: text });
    setText("");
    setSaved(true);
  };
  return (
    <div className="pr-field">
      <label htmlFor={id}>{label}</label>
      <div className="pr-row">
        <input
          id={id}
          type="password"
          className="pr-input"
          autoComplete="off"
          value={text}
          disabled={disabled}
          onChange={(e) => {
            setText(e.currentTarget.value);
            setSaved(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
        <button type="button" className="button" disabled={disabled || text === ""} onClick={save}>
          {t("password.save")}
        </button>
      </div>
      <span className="pr-counter" role="status">
        {saved ? t("password.saved") : ""}
      </span>
    </div>
  );
}

/** A native "open file" dialog; with the mock engine (browser dev) a prompt asks for a path. */
async function pickFile(title: string, extensions: string[]): Promise<string | null> {
  if (getApi().kind !== "tauri") {
    const answer = window.prompt(title, "");
    return answer === null || answer.trim() === "" ? null : answer.trim();
  }
  // An empty filter would hide every file on some platforms, so "any file" passes none.
  if (extensions.length === 0) return open({ title, multiple: false, directory: false });
  return open({ title, multiple: false, directory: false, filters: [{ name: title, extensions }] });
}

/** Lets the user pick one file; the host turns the path into a handle only `net.http` can use. */
export function PluginFilePicker({ label, accept, action, disabled }: Of<"filePicker">) {
  const { send } = useRender();
  const { t } = useTranslation("pluginSlots");
  const [name, setName] = useState<string | null>(null);
  const pick = async () => {
    const extensions = accept.map((ext) => ext.replace(/^\./, "")).filter((ext) => ext !== "");
    const path = await pickFile(label, extensions);
    if (path === null) return;
    setName(path.split(/[\\/]/).pop() ?? path);
    send(action, { type: "file", path });
  };
  return (
    <div className="pr-field">
      <span>{label}</span>
      <div className="pr-row">
        <button
          type="button"
          className="button"
          disabled={disabled}
          aria-label={`${label}: ${t("file.choose")}`}
          onClick={() => {
            pick().catch((error: unknown) => {
              console.error("Picking a file failed", error);
            });
          }}
        >
          {t("file.choose")}
        </button>
        <span className="pr-counter">{name ?? t("file.none")}</span>
      </div>
    </div>
  );
}
