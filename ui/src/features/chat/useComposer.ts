/**
 * State and actions of the message input: text, an attachment tray of images, sending and inline errors.
 * Builds the HTML that goes to the server and enforces its size limits before sending.
 */
import { useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import type { SendTarget } from "./conversationInfo";
import { readDroppedFiles } from "./fileDrop";
import { FULL_ATTRIBUTE_RESERVE, fullImageTag, keepFullImage } from "./fullImage";
import { imageTag, prepareImage, type MessageLimits } from "./images";
import { composeCards } from "./cards";
import { markdownToHtml, plainTextToHtml } from "./markdown";
import { quoteHtml, type ReplyQuote } from "./reply";
import { sendOwn } from "./sendMessage";

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
  /** Reference of the kept original when the picture had to be shrunk (FR-1.60c). */
  fullImage: string | null;
}

const noop = (): void => undefined;

function bodyHtml(text: string, limits: MessageLimits): string {
  // Cards go out as HTML official clients can read (issue #72); without HTML the typed BB code
  // stays text, which Mumcord still renders as a card.
  if (!limits.allowHtml) return plainTextToHtml(text);
  return composeCards(text, markdownToHtml) ?? markdownToHtml(text);
}

export interface Composer {
  text: string;
  setText: (text: string) => void;
  attachments: Attachment[];
  /** Adds images to the tray; processing runs one by one in the background. */
  addFiles: (files: readonly File[]) => void;
  /** Adds image files dropped from the file manager (native drag and drop) by their paths. */
  addDroppedPaths: (paths: readonly string[]) => Promise<void>;
  /** Adds image files copied in the file manager, read from the system clipboard (issue #50). */
  addCopiedFiles: () => Promise<void>;
  removeAttachment: (id: number) => void;
  error: string | null;
  /** Reports a problem the file itself cannot explain, such as a clipboard image the webview cannot hand over. */
  showError: (message: string) => void;
  /**
   * Sends the text and the images: the input empties at once and the messages show greyed until
   * confirmed (FR-2.74).
   */
  submit: () => void;
  /** Sends a link as its own message right away (GIF favorites); the typed text stays. */
  sendLink: (url: string) => Promise<void>;
}

/**
 * `reply` is the message being replied to (FR-2.60): its quote goes in front of the text, and
 * `onReplied` runs once the reply is sent.
 */
export function useComposer(
  target: SendTarget | null,
  limits: MessageLimits,
  reply: ReplyQuote | null = null,
  onReplied: () => void = noop,
): Composer {
  const { t } = useTranslation("chat");
  const [text, setTextState] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
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
      const result = await prepareImage(
        file,
        limits.image - imageTag("").length - FULL_ATTRIBUTE_RESERVE,
      );
      const fullImage = result.ok ? await keepFullImage(file, result.dataUrl) : null;
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
                fullImage,
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
          fullImage: null,
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

  const addCopiedFiles = useCallback(async () => {
    let paths: string[];
    try {
      paths = await getApi().clipboardFiles();
    } catch (err) {
      console.error("reading the copied files failed", err);
      setError(t("errors.imageUnreadable"));
      return;
    }
    await addDroppedPaths(paths);
  }, [addDroppedPaths, t]);

  const removeAttachment = useCallback(
    (id: number) => {
      removed.current.add(id);
      drop(id);
      setError(null);
    },
    [drop],
  );

  const showError = useCallback((message: string) => {
    setError(message);
  }, []);

  const tooLong =
    limits.text > 0 ? t("errors.tooLong", { max: limits.text }) : t("errors.tooLongNoLimit");

  const submit = useCallback(() => {
    if (target === null) return;
    if (attachments.some((item) => item.status === "processing")) return;
    const typed = bodyHtml(text, limits);
    if (typed === "" && attachments.length === 0) return;
    // The quote needs text to go with; images alone are sent without it.
    const body =
      typed !== "" && reply !== null ? quoteHtml(reply, limits.allowHtml) + typed : typed;
    if (body !== "" && limits.text > 0 && body.length > limits.text) {
      setError(t("errors.tooLong", { max: limits.text }));
      return;
    }
    setError(null);
    const serverId = shownServerId();
    // The server limit is per message, so the text and every image are separate messages in order.
    if (body !== "") {
      sendOwn(t, serverId, target, body, tooLong);
      setTextState("");
      if (reply !== null) onReplied();
    }
    for (const item of attachments) {
      sendOwn(
        t,
        serverId,
        target,
        fullImageTag(item.dataUrl, item.fullImage),
        t("errors.imageTooLarge"),
      );
      removed.current.add(item.id);
    }
    if (attachments.length > 0) setAttachments([]);
  }, [attachments, limits, onReplied, reply, t, target, text, tooLong]);

  const sendLink = useCallback(
    (url: string) => {
      if (target === null) return Promise.resolve();
      const html = bodyHtml(url, limits);
      if (limits.text > 0 && html.length > limits.text) {
        setError(t("errors.tooLong", { max: limits.text }));
        return Promise.reject(new Error("link too long"));
      }
      setError(null);
      sendOwn(t, shownServerId(), target, html, tooLong);
      return Promise.resolve();
    },
    [limits, t, target, tooLong],
  );

  return {
    text,
    setText,
    attachments,
    addFiles,
    addDroppedPaths,
    addCopiedFiles,
    removeAttachment,
    error,
    showError,
    submit,
    sendLink,
  };
}
