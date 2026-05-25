# Icon assets

The marketplace requires a **128×128 PNG** at `media/icon.png`. This repo
ships only the SVG used for the activity-bar sidebar (`media/icon.svg`).

Before publishing the first .vsix, generate a polished PNG. Options:

1. Open `media/icon.svg` in Figma / Inkscape / Affinity / Illustrator.
2. Style it however you'd like — gradient, glow, etc. — keep it
   readable at 32×32 and 128×128.
3. Export as `media/icon.png` at exactly 128×128 with transparency.

Until then, `package.json` references `media/icon.png` but the file is
absent; this is intentional — building a .vsix without the PNG will fail
loudly so you can't accidentally publish an unbranded extension.
