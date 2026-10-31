import { useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { toEngineError } from "../../api";
import { Avatar } from "../members/Avatar";
import { Dialog } from "../servers/Dialog";
import { displayName } from "../servers/format";
import { useServers } from "../servers/store";
import { closeBlockDialog, useBlockTarget, type BlockTarget } from "./blockDialogStore";
import { blockPerson, useBlockedMessages } from "./blockedStore";
import "./blocking.css";

/** Renders the "Block …?" confirmation while one is open; mount once. */
export function BlockDialogHost() {
  const target = useBlockTarget();
  if (!target) return null;
  return <BlockDialog target={target} />;
}

function BlockDialog({ target }: { target: BlockTarget }) {
  const { t } = useTranslation("members");
  const mode = useBlockedMessages();
  const { servers } = useServers();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const warningId = useId();
  const { name, serverId, hasCert, how, opener } = target;
  const saved = servers.find((s) => s.id === serverId);
  const server = saved ? displayName(saved) : "";

  const close = () => {
    closeBlockDialog();
    if (opener?.isConnected) opener.focus();
  };
  const confirm = () => {
    close();
    blockPerson(serverId, how, name).then(
      () => {
        announce(t("block.done", { name }));
      },
      (error: unknown) => {
        console.error("Blocking a person failed", toEngineError(error).code);
        announce(t("block.failed", { name }), "assertive");
      },
    );
  };

  return (
    <Dialog
      title={t("block.title", { name })}
      onClose={close}
      initialFocus={cancelRef}
      describedBy={warningId}
      className="block-dialog"
    >
      <span className="block-dialog__avatar">
        <Avatar name={name} size="lg" />
      </span>
      {server !== "" && <p className="block-dialog__server">{t("block.onServer", { server })}</p>}
      <h3 className="block-dialog__heading">{t("block.whatHappens")}</h3>
      <ul className="block-dialog__list">
        <li>{t("block.private", { name })}</li>
        <li>{t(mode === "hide" ? "block.channelHidden" : "block.channelCollapsed", { name })}</li>
        <li>{t("block.voice", { name })}</li>
        <li>{t("block.data", { name })}</li>
        <li>{t(hasCert ? "block.followsCert" : "block.nameOnly", { name, server })}</li>
      </ul>
      <p id={warningId} className="block-dialog__warning">
        {t("block.warning", { name })}
      </p>
      <div className="servers-dialog__actions">
        <button ref={cancelRef} type="button" className="button" onClick={close}>
          {t("block.cancel")}
        </button>
        <button
          type="button"
          className="button block-dialog__confirm"
          data-testid="block-confirm"
          onClick={confirm}
        >
          {t("block.confirm", { name })}
        </button>
      </div>
    </Dialog>
  );
}
