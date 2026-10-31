let opener: ((channelId: number) => void) | null = null;

/** Registers the mounted `ChannelAudioHost`; `null` when it unmounts. */
export function setChannelAudioOpener(fn: ((channelId: number) => void) | null): void {
  opener = fn;
}

/**
 * Opens "Audio for this channel" for a channel of the shown server, from anywhere (e.g. the
 * channel tree's right-click menu). Needs `ChannelAudioHost` mounted once in the app shell.
 */
export function openChannelAudioDialog(channelId: number): void {
  if (opener === null) {
    console.warn("The channel audio dialog could not be opened: its host is not mounted");
    return;
  }
  opener(channelId);
}
