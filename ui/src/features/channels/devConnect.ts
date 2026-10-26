/**
 * Dev-only helper for checking the channel UI in a browser before the server bar (B19) exists:
 * with the mock forced (`VITE_MUMCORD_MOCK=1`) and `?mockConnect` in the URL, connects to the
 * first saved mock server. Only loaded behind `import.meta.env.DEV`, so production never has it.
 */
import { getApi } from "../../api";

let done = false;

export function devAutoConnect(): void {
  if (done || import.meta.env.VITE_MUMCORD_MOCK !== "1") return;
  if (!new URLSearchParams(window.location.search).has("mockConnect")) return;
  done = true;
  const api = getApi();
  void api
    .serversList()
    .then((servers) => (servers[0] ? api.connect(servers[0].id, null, null) : null))
    .catch((error: unknown) => {
      console.error("Dev auto-connect failed", error);
    });
}
