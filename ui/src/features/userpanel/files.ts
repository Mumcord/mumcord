import { open, save } from "@tauri-apps/plugin-dialog";

/** Native file pickers; kept in one place so tests can replace them. */
export async function pickImagePath(title: string): Promise<string | null> {
  const picked = await open({
    title,
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
  });
  return picked;
}

export async function pickSavePath(title: string, defaultPath: string): Promise<string | null> {
  return save({ title, defaultPath, filters: [{ name: "PKCS#12", extensions: ["p12"] }] });
}
