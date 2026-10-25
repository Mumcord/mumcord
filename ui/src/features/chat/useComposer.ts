/**
 * State and actions of the message input: text, an attachment tray of images, sending and inline errors.
 * Builds the HTML that goes to the server and enforces its size limits before sending.
 */
import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ChatMessage } from "../../api";
import { addSentMessage } from "./chatStore";
import type { SendTarget } from "./conversationInfo";
import { describeSendError } from "./errors";
import { readDroppedFiles } from "./fileDrop";
import { imageTag, prepareImage, type MessageLimits } from "./images";
import { markdownToHtml, plainTextToHtml } from "./markdown";

/** One image in the attachment tray: being processed, or ready to send. */
export interface Attachment {
  id: number;
  name: string;
  status: "processing" | "ready";
  /** `data:` URL, already fitted to the server limit; empty while processing. */
  dataUrl: string;
  width: number;
  height: number;
  /** True when the picture had to be made smaller to fit the server limit. */
  scaled: boolean;
}

function bodyHtml(text: string, limits: MessageLimits): string {
  return limits.allowHtml ? markdownToHtml(text) : plainTextToHtml(text);
}

function send(target: SendTarget, html: string): Promise<ChatMessage> {
  const api = getApi();
  return target.kind === "channel"
    ? api.sendChannelMessage(target.channelId, html)
    : api.sendPrivateMessage(target.session, html);
}

export interface Composer {
  text: string;
  setText: (text: string) => void;
  attachments: Attachment[];
  /** Adds images to the tray; processing runs one by one in the background. */
  addFiles: (files: readonly File[]) => void;
  /** Adds image files dropped from the file manager (native drag and drop) by their paths. */
  addDroppedPaths: (paths: readonly string[]) => Promise<void>;
  removeAttachment: (id: number) => void;
  busy: boolean;
  error: string | null;
  submit: () => Promise<void>;
  /** Sends a link as its own message right away (GIF favorites); the typed text stays. */
  sendLink: (url: string) => Promise<void>;
}

export function useComposer(target: SendTarget | null, limits: MessageLimits): Composer {
  const { t } = useTranslation("chat");
  const [text, setTextState] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nextId = useRef(1);
  // Files wait here so only one image is decoded at a time (keeps memory and the UI calm).
  const queue = useRef<Promise<void>>(Promise.resolve());
  const removed = useRef(new Set<number>());

  const setText = useCallback((next: string) => {
    setTextState(next);
    setError(null);
  }, []);

  const drop = useCallback((id: number) => {
    setAttachments((list) => list.filter((item) => item.id !== id));
  }, []);

  const process = useCallback(
    async (id: number, file: File) => {
      if (removed.current.has(id)) return;
      const result = await prepareImage(file, limits.image - imageTag("").length);
      if (removed.current.has(id)) return;
      if (!result.ok) {
        drop(id);
        const key = {
          notImage: "errors.onlyImages",
          unreadable: "errors.imageUnreadable",
          tooLarge: "errors.imageTooLarge",
          sourceTooLarge: "errors.sourceTooLarge",
        }[result.reason];
        setError(t(key, { name: file.name }));
        return;
      }
      setAttachments((list) =>
        list.map((item) =>
          item.id === id
            ? {
                ...item,
                status: "ready",
                dataUrl: result.dataUrl,
                width: result.width,
                height: result.height,
                scaled: result.scaled,
              }
            : item,
        ),
      );
    },
    [drop, limits.image, t],
  );

  const addFiles = useCallback(
    (files: readonly File[]) => {
      setError(null);
      if (files.length === 0) return;
      if (!limits.allowHtml) {
        setError(t("errors.imagesNotAllowed"));
        return;
      }
      const images = files.filter((file) => file.type.startsWith("image/"));
      if (images.length < files.length) setError(t("errors.onlyImages"));
      const added = images.map((file) => ({ id: nextId.current++, file }));
      if (added.length === 0) return;
      setAttachments((list) => [
        ...list,
        ...added.map(({ id, file }): Attachment => ({
          id,
          name: file.name || t("composer.unnamedImage"),
          status: "processing",
          dataUrl: "",
          width: 0,
          height: 0,
          scaled: false,
        })),
      ]);
      for (const { id, file } of added) {
        queue.current = queue.current.then(() => process(id, file));
      }
    },
    [limits.allowHtml, process, t],
  );

  const addDroppedPaths = useCallback(
    async (paths: readonly string[]) => {
      setError(null);
      if (paths.length === 0) return;
      if (!limits.allowHtml) {
        setError(t("errors.imagesNotAllowed"));
        return;
      }
      const { files, failures } = await readDroppedFiles(paths);
      addFiles(files);
      const failure = failures[0];
      if (failure !== undefined) {
        const key = {
          notImage: "errors.onlyImages",
          sourceTooLarge: "errors.sourceTooLarge",
          unreadable: "errors.imageUnreadable",
        }[failure.reason];
        setError(t(key, { name: failure.name }));
      }
    },
    [addFiles, limits.allowHtml, t],
  );

  const removeAttachment = useCallback(
    (id: number) => {
      removed.current.add(id);
      drop(id);
      setError(null);
    },
    [drop],
  );

  const submit = useCallback(async () => {
    if (busy || target === null) return;
    if (attachments.some((item) => item.status === "processing")) return;
    const body = bodyHtml(text, limits);
    if (body === "" && attachments.length === 0) return;
    if (body !== "" && limits.text > 0 && body.length > limits.text) {
      setError(t("errors.tooLong", { max: limits.text }));
      return;
    }
    setBusy(true);
    setError(null);
    let sending = body !== "" ? "text" : "image";
    try {
      // The server limit is per message, so the text and every image are separate messages in order.
      if (body !== "") {
        addSentMessage(await send(target, body));
        setTextState("");
        sending = "image";
      }
      for (const item of attachments) {
        addSentMessage(await send(target, imageTag(item.dataUrl)));
        removed.current.add(item.id);
        drop(item.id);
      }
    } catch (err) {
      const tooLarge =
        sending === "text"
          ? limits.text > 0
            ? t("errors.tooLong", { max: limits.text })
            : t("errors.tooLongNoLimit")
          : t("errors.imageTooLarge");
      setError(describeSendError(t, err, tooLarge));
    } finally {
      setBusy(false);
    }
  }, [attachments, busy, drop, limits, t, target, text]);

  const sendLink = useCallback(
    async (url: string) => {
      if (busy || target === null) return;
      const html = bodyHtml(url, limits);
      if (limits.text > 0 && html.length > limits.text) {
        setError(t("errors.tooLong", { max: limits.text }));
        throw new Error("link too long");
      }
      setBusy(true);
      setError(null);
      try {
        addSentMessage(await send(target, html));
      } catch (err) {
        setError(describeSendError(t, err, t("errors.tooLongNoLimit")));
        throw err;
      } finally {
        setBusy(false);
      }
    },
    [busy, limits, t, target],
  );

  return {
    text,
    setText,
    attachments,
    addFiles,
    addDroppedPaths,
    removeAttachment,
    busy,
    error,
    submit,
    sendLink,
  };
}
