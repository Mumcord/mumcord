/**
 * Whether the page Mumcord's own window shows is a website page (issue #419).
 *
 * A player inside the app is framed by that page, so its origin is part of the chain the player
 * sees. Tauri loads the UI from `tauri://localhost` on Linux and macOS and from
 * `http(s)://tauri.localhost` on Windows; `npm run tauri dev` serves it over http. Twitch's player
 * only runs under http(s) pages, so a card there has to hand the stream to the browser.
 *
 * The development server's origin counts as a website: the player really can run there.
 */
export function appPageIsWebPage(): boolean {
  const protocol = window.location.protocol;
  return protocol === "http:" || protocol === "https:";
}
