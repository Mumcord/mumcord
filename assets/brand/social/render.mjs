// Exports every *.html share image in this folder and in its set folders (neon/, light/, glass/)
// to a PNG of the same name, at the size in the file name (e.g. og-1200x630.html → og-1200x630.png).
//
// Usage: node render.mjs [folder ...]   (no folder: this folder and every subfolder with HTML files)
// Needs playwright-core and a Chromium build; point to them with PLAYWRIGHT_CORE (module path) and
// CHROMIUM (executable path) if they are not resolvable / installed by Playwright. Fonts load from
// Google Fonts, so the machine needs network access. Optimise the PNGs afterwards, e.g. with
// `pngquant --force --ext .png 256 *.png` (or lossless: `oxipng -o max --strip safe *.png`).
import { readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { chromium } = await import(process.env.PLAYWRIGHT_CORE ?? "playwright-core");

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM,
  args: ["--allow-file-access-from-files"],
});
try {
  const htmlIn = (dir) => readdirSync(dir).filter((f) => f.endsWith(".html")).sort();
  const dirs = process.argv.length > 2
    ? process.argv.slice(2).map((d) => join(here, d))
    : [here, ...readdirSync(here, { withFileTypes: true })
        .filter((e) => e.isDirectory() && htmlIn(join(here, e.name)).length > 0)
        .map((e) => join(here, e.name))];
  for (const [dir, file] of dirs.flatMap((d) => htmlIn(d).map((f) => [d, f]))) {
    const size = /-(\d+)x(\d+)\.html$/.exec(file);
    if (!size) throw new Error(`${file}: name must end in -<width>x<height>.html`);
    const [width, height] = [Number(size[1]), Number(size[2])];
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
    await page.goto(pathToFileURL(join(dir, file)).href, { waitUntil: "networkidle" });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: join(dir, file.replace(/\.html$/, ".png")) });
    await page.close();
    console.log(`rendered ${join(dir, file).slice(here.length + 1)}`);
  }
} finally {
  await browser.close();
}
