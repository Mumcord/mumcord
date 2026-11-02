/** The parsed CHANGELOG.md, generated at build time by the `mumcord-changelog` plugin (vite.config.ts). */
declare module "virtual:changelog" {
  const versions: import("./features/changelog/parseChangelog").ChangelogVersion[];
  export default versions;
}
