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
import { isDeafened, isMuted } from "../channels/statusText";
import { useServerState } from "../channels/serverStore";
import "./profilePreview.css";
import { Avatar } from "./Avatar";
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
  children,
}: {
  session: number;
  children: ReactNode;
}) {
  return (
    // Not interactive itself: it only observes hover and focus of the person inside.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <span
      className="profile-preview-trigger"
      onPointerEnter={(event) => {
        if (isHoverPointer(event)) requestPreview(session, anchorOf(event.currentTarget));
      }}
      onPointerLeave={(event) => {
        if (isHoverPointer(event)) releasePreview();
      }}
      // Closes before the click lands, so the full card opens without the preview in the way.
      onPointerDown={closePreview}
      onFocus={(event: FocusEvent<HTMLSpanElement>) => {
        previewOnFocus(session, event.currentTarget, event.target);
      }}
      onBlur={releasePreview}
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
  const user = state.users.get(open.session);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!user) closePreview();
  }, [user]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closePreview();
    };
    const onScroll = (event: Event) => {
      // Scrolling a long comment inside the preview keeps it open.
      if (!(event.target instanceof Node && ref.current?.contains(event.target))) closePreview();
    };
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", closePreview);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", closePreview);
    };
  }, []);

  // Runs after every render: the size changes when the comment arrives.
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    if (!open.anchor.isConnected) {
      closePreview();
      return;
    }
    const { left, top } = placeNextTo(
      open.anchor.getBoundingClientRect(),
      element.offsetWidth,
      element.offsetHeight,
      window.innerWidth,
      window.innerHeight,
    );
    element.style.left = `${String(left)}px`;
    element.style.top = `${String(top)}px`;
  });

  if (!user) return null;
  return (
    <div
      ref={ref}
      id={PREVIEW_ID}
      role="tooltip"
      className="profile-preview"
      onPointerEnter={holdPreview}
      onPointerLeave={releasePreview}
    >
      <PreviewBody key={user.session} user={user} talking={state.talking.has(user.session)} />
    </div>
  );
}

function PreviewBody({ user, talking }: { user: User; talking: boolean }) {
  const { t } = useTranslation("members");
  const { t: tStatus } = useTranslation("channels");
  const { profile } = usePersonProfile(user.session);
  // Sanitized here like on the full card: server comments are untrusted.
  const comment = profile?.comment ? DOMPurify.sanitize(profile.comment) : "";
  const voice = [
    talking && tStatus("status.talking"),
    isDeafened(user) ? tStatus("status.deafened") : isMuted(user) && tStatus("status.muted"),
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
