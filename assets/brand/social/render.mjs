// Exports every *.html share image in this folder to a PNG of the same name, at the size in the
// file name (e.g. og-1200x630.html → og-1200x630.png).
//
// Usage: node render.mjs
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
  for (const file of readdirSync(here).filter((f) => f.endsWith(".html")).sort()) {
    const size = /-(\d+)x(\d+)\.html$/.exec(file);
    if (!size) throw new Error(`${file}: name must end in -<width>x<height>.html`);
    const [width, height] = [Number(size[1]), Number(size[2])];
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
    await page.goto(pathToFileURL(join(here, file)).href, { waitUntil: "networkidle" });
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: join(here, file.replace(/\.html$/, ".png")) });
    await page.close();
    console.log(`rendered ${file}`);
  }
} finally {
  await browser.close();
}
