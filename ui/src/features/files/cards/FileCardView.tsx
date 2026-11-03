import type { FileCard } from "../../../api";
import "../../chat/mediaCards.css";
import "../../plugins/prompt/progress.css";
import "./fileCards.css";
import { OwnFileCard } from "./OwnFileCard";
import { ReceivedFileCard } from "./ReceivedFileCard";

/**
 * A file offer in the chat (FR-7.16, FR-7.19): replaces the plain text line official Mumble
 * clients see. Built from the audio card frame, like the mockups of #348.
 */
export function FileCardView({ card }: { card: FileCard }) {
  return card.own ? <OwnFileCard card={card} /> : <ReceivedFileCard card={card} />;
}
