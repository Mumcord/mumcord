/** Test helpers for the channels and members features. */
import { createMockApi, setApi, type MockApi } from "../../api";
import { applyLanguage } from "../../i18n";
import { resetSelection } from "./selection";
import { resetServerStore } from "./serverStore";

/** Fresh mock engine, connected to the seeded server (own user in Lobby). */
export async function connectedMock(): Promise<MockApi> {
  resetServerStore();
  resetSelection();
  await applyLanguage("en");
  const api = createMockApi();
  setApi(api);
  await api.connect(1, null);
  return api;
}

export function resetAll(): void {
  resetServerStore();
  resetSelection();
  setApi(undefined);
}

/** jsdom has no modal dialogs; a minimal stand-in keeps `showModal()` callable. */
export function stubDialog(): void {
  const proto = HTMLDialogElement.prototype as Partial<HTMLDialogElement>;
  proto.showModal ??= function showModal(this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  proto.close ??= function close(this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
}
