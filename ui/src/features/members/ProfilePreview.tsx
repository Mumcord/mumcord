/**
 * Read-only profile preview on hover or keyboard focus (FR-1.18, QA.21): avatar, name, comment,
 * registered marker and voice state, placed next to the person. A click still opens the full
 * profile card. Wrap a person in `ProfilePreviewTrigger`; `ProfilePreviewHost` is rendered once.
 */
import DOMPurify from "dompurify";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useSyncExternalStore,
  type FocusEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { VOICE_STATE_KEY, voiceState } from "../channels/statusText";
import { useServerState } from "../channels/serverStore";
import "./profilePreview.css";
import { Avatar } from "./Avatar";
import { openPersonMenuFromEvent, openPersonMenuFromKey } from "./personMenuStore";
import { findPerson } from "./personLookup";
import { placeNextTo } from "./placement";
import {
  closePreview,
  currentPreview,
  holdPreview,
  PREVIEW_ID,
  previewOnFocus,
  releasePreview,
  requestPreview,
  subscribePreview,
  type OpenPreview,
} from "./profilePreviewStore";
import { usePersonProfile } from "./usePersonProfile";

/** Touch has no hover; a tap is a click and opens the full card. */
const isHoverPointer = (event: PointerEvent) => event.pointerType !== "touch";

/**
 * Element to place the preview beside: inside a tree row, the whole row, so the preview never
 * covers the rest of it (status icons); elsewhere the trigger itself.
 */
const anchorOf = (trigger: HTMLElement) =>
  trigger.closest<HTMLElement>('[role="treeitem"]') ?? trigger;

/** Reports hover and keyboard focus on a person to the shared preview. Stateless, so cheap per row. */
export function ProfilePreviewTrigger({
  session,
  name,
  children,
}: {
  /** `null` when the person has no live session (chat history). */
  session: number | null;
  /** Stored sender name (chat): finds the person again and labels the limited preview. */
  name?: string;
  children: ReactNode;
}) {
  return (
    // Not interactive itself: it only observes hover and focus of the person inside.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <span
      className="profile-preview-trigger"
      data-person-session={session ?? undefined}
      onPointerEnter={(event) => {
        if (isHoverPointer(event)) {
          requestPreview(session, anchorOf(event.currentTarget), undefined, name);
        }
      }}
      onPointerLeave={(event) => {
        if (isHoverPointer(event)) releasePreview();
      }}
      // Closes before the click lands, so the full card opens without the preview in the way.
      onPointerDown={closePreview}
      onFocus={(event: FocusEvent<HTMLSpanElement>) => {
        previewOnFocus(session, event.currentTarget, event.target, name);
      }}
      onBlur={releasePreview}
      onContextMenu={(event) => {
        openPersonMenuFromEvent(session, event, name);
      }}
      onKeyDown={(event) => {
        openPersonMenuFromKey(session, event, name);
      }}
    >
      {children}
    </span>
  );
}

/** Renders the open preview, if any, into the document body. */
export function ProfilePreviewHost() {
  const open = useSyncExternalStore(subscribePreview, currentPreview);
  // Nothing may stay open (or keep aria-describedby) once the host is gone.
  useEffect(() => closePreview, []);
  if (!open) return null;
  return createPortal(<Preview open={open} />, document.body);
}

function Preview({ open }: { open: OpenPreview }) {
  const state = useServerState();
  const user = findPerson(state.users, open.session, open.name);
  const ref = useRef<HTMLDivElement>(null);
  // The trigger may be re-created by a re-render (tree rows, chat list); follow it by session.
  const anchorRef = useRef(open.anchor);
  useLayoutEffect(() => {
    anchorRef.current = open.anchor;
  }, [open.anchor]);
  const placeRef = useRef<() => void>(() => undefined);

  // Without a stored name there is nothing to show for a person who left.
  const gone = !user && open.name === undefined;
  useEffect(() => {
    if (gone) closePreview();
  }, [gone]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closePreview();
    };
    const onScroll = (event: Event) => {
      const target = event.target;
      if (target === document || target instanceof Window) {
        closePreview();
        return;
      }
      if (!(target instanceof Node) || ref.current?.contains(target)) return;
      // Only the page itself closes it; a container scrolling by itself (the chat list following
      // a new message) just moves the person, so the preview follows.
      if (target.contains(anchorRef.current)) placeRef.current();
    };
    // The user scrolling with the wheel elsewhere closes it, like moving away.
    const onWheel = (event: Event) => {
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) closePreview();
    };
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("wheel", onWheel, { capture: true, passive: true });
    window.addEventListener("blur", closePreview);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("wheel", onWheel, true);
      window.removeEventListener("blur", closePreview);
    };
  }, []);

  function place() {
    const element = ref.current;
    if (!element) return;
    let anchor = anchorRef.current;
    if (!anchor.isConnected) {
      const again =
        open.session === null
          ? null
          : document.querySelector<HTMLElement>(
              `.profile-preview-trigger[data-person-session="${String(open.session)}"]`,
            );
      if (!again) {
        closePreview();
        return;
      }
      anchor = anchorOf(again);
      anchorRef.current = anchor;
    }
    const { left, top } = placeNextTo(
      anchor.getBoundingClientRect(),
      element.offsetWidth,
      element.offsetHeight,
      window.innerWidth,
      window.innerHeight,
    );
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
  }

  // Runs after every render: the size changes when the comment arrives.
  useLayoutEffect(() => {
    placeRef.current = place;
    place();
  });

  if (gone) return null;
  return (
    <div
      ref={ref}
      id={PREVIEW_ID}
      role="tooltip"
      className="profile-preview"
      onPointerEnter={holdPreview}
      onPointerLeave={releasePreview}
    >
      {user ? (
        <PreviewBody key={user.session} user={user} talking={state.talking.has(user.session)} />
      ) : (
        <div className="profile-preview__header">
          <Avatar name={open.name ?? ""} src={null} size="lg" />
          <div className="profile-preview__identity">
            <p className="profile-preview__name">{open.name}</p>
          </div>
        </div>
      )}
    </div>
  );
}

function PreviewBody({ user, talking }: { user: User; talking: boolean }) {
  const { t } = useTranslation("members");
  const { t: tStatus } = useTranslation("channels");
  const { profile } = usePersonProfile(user.session);
  // Sanitized here like on the full card: server comments are untrusted.
  const comment = profile?.comment ? DOMPurify.sanitize(profile.comment) : "";
  const state = voiceState(user);
  const voice = [
    talking && tStatus("status.talking"),
    state && tStatus(VOICE_STATE_KEY[state]),
    user.localMute && tStatus("status.localMuted"),
  ].filter((part): part is string => typeof part === "string");

  return (
    <>
      <div className="profile-preview__header">
        <Avatar name={user.name} src={profile?.avatar ?? null} talking={talking} size="lg" />
        <div className="profile-preview__identity">
          <p className="profile-preview__name">{user.name}</p>
          <p className="profile-preview__meta">
            {user.registered ? t("registered") : t("unregistered")}
          </p>
          {voice.length > 0 && <p className="profile-preview__voice">{voice.join(", ")}</p>}
        </div>
      </div>
      {comment && (
        <div className="profile-preview__comment" dangerouslySetInnerHTML={{ __html: comment }} />
      )}
    </>
  );
}
