/**
 * The way out of a relay refusal (#601): a guest gets "How to register" (the user panel's register
 * dialog), a client the bot doesn't know yet gets "Reconnect", which keeps the channel.
 */
import { useTranslation } from "react-i18next";
import { reconnectKeepingChannel } from "./reconnect";
import { openRegisterDialog } from "../userpanel/registerPrompt";

export type RefusalHint = "register" | "reconnect";

export function RefusalAction({ hint, serverId }: { hint: RefusalHint; serverId: number }) {
  const { t } = useTranslation("p2p");
  return (
    <button
      type="button"
      className="button button--link"
      onClick={() => {
        if (hint === "register") {
          openRegisterDialog();
          return;
        }
        reconnectKeepingChannel(serverId).catch((error: unknown) => {
          console.error("Reconnecting for the relay failed", error);
        });
      }}
    >
      {hint === "register" ? t("refusalAction.howToRegister") : t("refusalAction.reconnect")}
    </button>
  );
}
