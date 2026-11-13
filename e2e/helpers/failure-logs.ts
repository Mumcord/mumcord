// Preserve only Mumcord's redacted diagnostic logs, never identities, keyrings or databases.
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";

export function saveFailureLogs(profile: string, destination: string, label: string): void {
  const source = join(profile, "data", "app.mumcord.client", "logs");
  if (!existsSync(source)) return;
  mkdirSync(destination, { recursive: true });
  for (const file of readdirSync(source, { withFileTypes: true })) {
    if (file.isFile() && /^mumcord\.\d{4}-\d{2}-\d{2}\.log$/.test(file.name)) {
      copyFileSync(join(source, file.name), join(destination, `${label}-${file.name}`));
    }
  }
}
