# Share images for social media

Ready-to-post images in the Mumcord look (same colours, fonts and logo as the app and the
GitHub social preview in `../social-preview.png`). Each image has an editable HTML source and an
exported PNG with the same name. The app screenshot comes from
`docs/images/screenshots/main-dark.png`.

| File | Use it for | Size | Safe zone (keep text inside) |
| --- | --- | --- | --- |
| `og-1200x630.png` | Facebook and LinkedIn link/post image, Open Graph (`og:image`) for websites | 1200×630 | 60 px margin on every side; some previews crop the sides, so the text sits on the left and the screenshot is decorative. |
| `instagram-square-1080x1080.png` | Instagram square post | 1080×1080 | Centre 810×1080: the profile grid crops square posts to 3:4. The feed shows the full image. |
| `instagram-portrait-1080x1350.png` | Instagram portrait post (best reach in the feed) | 1080×1350 | Centre 1012×1350 (profile grid crops to 3:4), plus 60 px margin. |
| `story-1080x1920.png` | Instagram/Facebook Story, Reel or TikTok cover | 1080×1920 | Keep clear of the top 250 px and the bottom 340 px (profile name, reply bar, captions). The grid shows the centre 1080×1440 of a Reel cover. |
| `x-post-1600x900.png` | X/Twitter post image | 1600×900 | Shown in full at 16:9; 60 px margin. |
| `facebook-cover-1640x624.png` | Facebook page cover | 1640×624 | Centre 1100×624: phones crop the left and right. On desktop the profile picture covers the lower-left corner. |
| `linkedin-banner-1584x396.png` | LinkedIn page or profile banner | 1584×396 | Right of 560 px and above the bottom 60 px: the profile photo or page logo covers the lower left, and phones crop the edges. |

All text is sized to stay readable when the image is shown at phone width.

## Style sets: neon, light and glass

Three more sets that lead with what Mumcord is for (voice chat for friends, gaming groups and
communities) and mention Mumble only in a small line. Each folder holds an HTML source and a PNG
for all seven formats above, with the same file names, sizes and safe zones.

| Folder | Look | Headline | Small line |
| --- | --- | --- | --- |
| `neon/` | Dark synthwave: neon glow, perspective grid, tilted dark-theme screenshot | "Talk. Share. Play together." | Works with any Mumble server |
| `light/` | Clean editorial: off-white, lots of space, light-theme screenshot | "Voice chat for your crew. Crystal clear." | Works with any Mumble server |
| `glass/` | 3D glass: gradient, glossy orbs, floating cards (screen share, chat, cameras) | "Crystal-clear voice for your community." | Built on the open Mumble protocol |

Which file goes where (same in every set):

| File | Use it for |
| --- | --- |
| `og-1200x630.png` | Facebook and LinkedIn link/post image, Open Graph (`og:image`) |
| `instagram-square-1080x1080.png` | Instagram square post |
| `instagram-portrait-1080x1350.png` | Instagram portrait post |
| `story-1080x1920.png` | Instagram/Facebook Story, Reel or TikTok cover |
| `x-post-1600x900.png` | X/Twitter post image |
| `facebook-cover-1640x624.png` | Facebook page cover |
| `linkedin-banner-1584x396.png` | LinkedIn page or profile banner |

- `shared/` holds what the sets share: `base.css` (colours, fonts, common text styles) and two
  crops of the app screenshots (`crop-chat.png` from `chat-images.png`, `crop-share.png` from
  `screen-share-viewer.png`). Each set's look lives in its own `style.css`.
- The full screenshots come from `docs/images/screenshots/main-dark.png` (neon) and
  `main-light.png` (light).
- The webcam pictures in `glass/cameras/` show fictional people, not real persons. They are
  licensed for commercial use without attribution.

## Editing and exporting

- `social.css` holds the shared look (colours from `ui/src/theme/tokens.css`, the Bricolage
  Grotesque and Instrument Sans fonts from Google Fonts).
- Each `*-<width>x<height>.html` file sets its own layout. Open it in a browser to preview.
- `render.mjs` exports every HTML file in this folder and in the set folders to a PNG with headless
  Chromium (pass folder names, e.g. `node render.mjs glass`, to render only those):

  ```sh
  PLAYWRIGHT_CORE=/path/to/playwright-core/index.mjs CHROMIUM=/path/to/chrome node render.mjs
  ```

  Then shrink the PNGs, e.g. `pngquant --force --ext .png 256 *.png`. The committed PNGs are
  quantised to 256 colours.
