/**
 * State and actions of the message input: text, one attached image, sending and inline errors.
 * Builds the HTML that goes to the server and enforces its size limits before sending.
 */
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ChatMessage } from "../../api";
import { addSentMessage } from "./chatStore";
import type { SendTarget } from "./conversationInfo";
import { describeSendError } from "./errors";
import { imageTag, prepareImage, type MessageLimits } from "./images";
import { markdownToHtml, plainTextToHtml } from "./markdown";

const IMAGE_SEPARATOR = "<br />";

function bodyHtml(text: string, limits: MessageLimits): string {
  return limits.allowHtml ? markdownToHtml(text) : plainTextToHtml(text);
}

function messageHtml(body: string, image: string | null): string {
  if (image === null) return body;
  const tag = imageTag(image);
  return body === "" ? tag : `${body}${IMAGE_SEPARATOR}${tag}`;
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
  /** `data:` URL of the attached image, already fitted to the server limit. */
  image: string | null;
  addImage: (file: Blob) => Promise<void>;
  removeImage: () => void;
  busy: boolean;
  error: string | null;
  submit: () => Promise<void>;
}

export function useComposer(target: SendTarget | null, limits: MessageLimits): Composer {
  const { t } = useTranslation("chat");
  const [text, setTextState] = useState("");
  const [image, setImage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const setText = useCallback((next: string) => {
    setTextState(next);
    setError(null);
  }, []);

  const addImage = useCallback(
    async (file: Blob) => {
      setError(null);
      if (!limits.allowHtml) {
        setError(t("errors.imagesNotAllowed"));
        return;
      }
      // Leave room for the text typed so far and the surrounding tags.
      const overhead = bodyHtml(text, limits).length + IMAGE_SEPARATOR.length + imageTag("").length;
      setBusy(true);
      const result = await prepareImage(file, limits.image - overhead);
      setBusy(false);
      if (result.ok) {
        setImage(result.dataUrl);
      } else {
        setError(
          t(
            result.reason === "notImage"
              ? "errors.notImage"
              : result.reason === "unreadable"
                ? "errors.imageUnreadable"
                : "errors.imageTooLarge",
          ),
        );
      }
    },
    [limits, t, text],
  );

  const removeImage = useCallback(() => {
    setImage(null);
    setError(null);
  }, []);

  const submit = useCallback(async () => {
    if (busy || target === null) return;
    const html = messageHtml(bodyHtml(text, limits), image);
    if (html === "") return;
    const limit = image === null ? limits.text : limits.image;
    let tooLarge = t("errors.imageWithTextTooLong");
    if (image === null) {
      tooLarge =
        limits.text > 0 ? t("errors.tooLong", { max: limits.text }) : t("errors.tooLongNoLimit");
    }
    if (limit > 0 && html.length > limit) {
      setError(tooLarge);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      addSentMessage(await send(target, html));
      setTextState("");
      setImage(null);
    } catch (err) {
      setError(describeSendError(t, err, tooLarge));
    } finally {
      setBusy(false);
    }
  }, [busy, image, limits, t, target, text]);

  return { text, setText, image, addImage, removeImage, busy, error, submit };
}
