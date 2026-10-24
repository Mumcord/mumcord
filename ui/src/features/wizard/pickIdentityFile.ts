import { open } from "@tauri-apps/plugin-dialog";
import { getApi } from "../../api";

/**
 * Asks the user for a PKCS#12 identity file and returns its path, or `null` when cancelled.
 * The native dialog only exists inside Tauri; the in-memory mock (dev/test) gets a fake path.
 */
export async function pickIdentityFile(filterName: string): Promise<string | null> {
  if (getApi().kind === "mock") return "identity.p12";
  const path = await open({
    multiple: false,
    directory: false,
    filters: [{ name: filterName, extensions: ["p12", "pfx"] }],
  });
  return path;
}
