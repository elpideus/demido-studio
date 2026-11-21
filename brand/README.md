# Demido Studio brand kit

Draft 0.1, for the owner's review. The rules and parts every Demido screen is built from.

**See it:** run `pnpm brand:serve` and open <http://127.0.0.1:4173/kit/>, or open `brand/kit/index.html` from disk.
**Rebuild it:** `node brand/build.mjs` (no dependencies).

## What is here

| Path | Holds |
|---|---|
| `kit/` | The living brand kit page. Generated from `kit/src`; every specimen is the real CSS. |
| `logo/` | The owner's mark as clean vector masters, the small cut, app icon, favicon, tray icons. |
| `tokens/src/*.json` | Every value: colour, type, space, shape, motion. The single source of truth. |
| `tokens/dist/` | Generated: `tokens.css` (custom properties), `tokens.json` (with hex), `tokens.ts`, `glyphs.css`. |
| `css/` | Base styles and the reference components (`.d-*`). Conventions in `css/README.md`. |
| `fonts/` | Geologica and JetBrains Mono (SIL OFL 1.1), per-script subsets, with their licences. |
| `icons/state/` | The state glyphs: circles for work, eyes for models. |
| `icons/ui/` | The Lucide icons in use (ISC), with their licence. |
| `voice.md` | How Demido writes: Manual, Caveman, Reference. Vocabulary. |
| `explorations/lab/` | The type and colour tests behind the choices: reports and scripts. The comparison sheets are images the scripts rebuild; they are not in the repository (`explorations/lab/README.md`). |

## The brand in one page

1. **Quiet until it matters.** Neutral by default; colour is a signal, never decoration.
2. **Nothing hidden.** Detail is one step away, never removed.
3. **Read in one pass.** Written like an aircraft manual.
4. **State is always visible.** Circles are work, eyes are models.
5. **Motion explains.** Cause and effect; always interruptible.

- **Logo:** the owner's mark, one flat colour. Master from 40 px, small cut from 16 to 39 px.
- **Colour:** true neutral greys, dark by default. One accent, Demido green (`#68efaf` on dark, `#038a5a` on
  light), that means "a model is awake" and is the focus ring. The accent is a setting: Automatic follows
  the system accent when there is one, or the user picks any colour; every shade derives from one input,
  `--d-color-accent-source`. Four signals: success (always Demido green), caution, danger, info, each with its
  own glyph.
- **Type:** Geologica for text (weights tuned to 250 / 350 / 450), JetBrains Mono for data.
- **Shape:** 4 px grid, hairlines, soft corners (4 to 24 px), 1.5 px round-capped icons.
- **Primary action:** monochrome. It wears the logo's own white or black.

## Changing things

- A value: edit `tokens/src/*.json`, then `node brand/build.mjs`. Nothing else holds a raw value.
- A component: one file in `css/components/`. Add a file and the build picks it up.
- A kit section: one file in `kit/src/sections/` (plus optional CSS and JS of the same name).
- The accent: change `color.accent.source` in `tokens/src/color.json` (the default), or set
  `--d-color-accent-source` at runtime (what the Accent setting does). The kit page has the setting, live.

## Status

Proposals awaiting the owner's decision are marked on the kit page and listed in its last section, "For your
review". Not yet checked on Windows or macOS. Final lockup files (outlined letters) wait for the typeface decision.

## Licences

- Fonts: SIL Open Font License 1.1 (`fonts/OFL-*.txt`).
- UI icons: Lucide, ISC (`icons/ui/LICENSE.md`).
- The Demido name and logo: **not yet decided.** They are the owner's. Until a usage policy is written, do not
  assume the GPL covers them.
