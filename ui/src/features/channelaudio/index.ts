/**
 * Channel audio profiles (issue #171). `ChannelAudioDialog` edits one channel's profile; other
 * features open it with `openChannelAudioDialog(channelId)` (with `ChannelAudioHost` mounted once).
 */
export { ChannelAudioDialog } from "./ChannelAudioDialog";
export { ChannelAudioHost } from "./ChannelAudioHost";
export { openChannelAudioDialog } from "./opener";
export {
  saveChannelAudioProfile,
  useActiveAudioProfile,
  useChannelAudioProfile,
  useChannelAudioProfiles,
} from "./channelAudioStore";
export {
  applyProfile,
  overrides,
  profileKind,
  profileSummary,
  routePatch,
  type ProfileField,
  type ProfileKind,
} from "./model";
