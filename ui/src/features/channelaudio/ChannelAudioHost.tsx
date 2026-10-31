import { useEffect, useState } from "react";
import { ChannelAudioDialog } from "./ChannelAudioDialog";
import { setChannelAudioOpener } from "./opener";

/** Mount point of the dialog opened with `openChannelAudioDialog`. */
export function ChannelAudioHost() {
  const [channelId, setChannelId] = useState<number | null>(null);

  useEffect(() => {
    setChannelAudioOpener(setChannelId);
    return () => {
      setChannelAudioOpener(null);
    };
  }, []);

  if (channelId === null) return null;
  return (
    <ChannelAudioDialog
      channelId={channelId}
      onClose={() => {
        setChannelId(null);
      }}
    />
  );
}
