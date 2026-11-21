# Demido Studio styles: conventions

`brand/css` is the reference implementation of the brand in CSS. The app may reuse these files directly or
re-implement them in its framework; either way the look, names and states defined here are the contract.

## Files

| File | Holds |
|---|---|
| `demido.css` | One import for everything. Fixes the layer order. |
| `base.css` | Element defaults, text styles (`.d-type-*`), focus, user preferences. |
| `components/*.css` | One component per file. Picked up automatically by the build (`components.css` is generated). |
| `../tokens/dist/tokens.css` | All values. Generated from `../tokens/src/*.json`. |
| `../tokens/dist/glyphs.css` | State glyphs as mask variables. Generated from `../icons/state/*.svg`. |

Build: `node brand/build.mjs`.

## Rules for a component file

1. **Header comment first**: the markup to use, the variants, the states. Someone must be able to use the
   component from the comment alone.
2. **Everything inside `@layer demido.components { }`.**
3. **Names**: block `.d-name`, element `.d-name__part`, variant `.d-name--variant`. State is an attribute, not a
   class: native (`:disabled`, `:checked`, `[open]`), ARIA (`[aria-pressed="true"]`, `[aria-selected="true"]`,
   `[aria-current]`) or `data-state="…"`. Private custom properties start with `--_`.
4. **Tokens only.** No raw colours, font sizes, radii, durations or z-indexes. Allowed literals: `0`, `1px` via
   `--d-stroke-hairline`, percentages, unitless ratios, `em` for things that must follow the text size.
   If a value is missing, add a token in `tokens/src` (with a `$description`) rather than hard-coding it.
5. **Both themes.** Never assume dark. Use semantic tokens (`--d-color-bg-surface`), never a specific lightness.
6. **Quiet by default.** Neutral surfaces and text. The accent colour (`--d-color-accent-*`) means "a model is
   live here" and is the focus ring; it is not a decoration and not the primary-button colour. It is Demido
   green by default but the user or the system can change it, so never assume its hue: use the tokens, and
   use `--d-color-accent-on` for anything drawn on an accent fill. Success, caution,
   danger and info colours appear only when that state is true, and always with a glyph or label as well.
7. **Interaction**
   - Hover styles go inside `@media (hover: hover) and (pointer: fine)`.
   - Pressable things respond on press: `:active { transform: scale(0.97) }` (or a pressed fill), duration
     `--d-duration-instant`.
   - Focus comes from `base.css` (`:focus-visible` outline). Do not remove it; set `border-radius` so it fits.
   - Pointer targets are at least `--d-size-hit`; extend small visuals with a pseudo-element.
   - Disabled: `opacity: var(--d-opacity-disabled); pointer-events: none`.
8. **Motion**
   - `transition` names its properties. Never `transition: all`.
   - Animate `transform`, `opacity`, colours. Not width, height, margin, padding (use `grid-template-rows`,
     `clip-path` or `scale` if size must change).
   - Enter with `--d-ease-out`; exits are the same curve and shorter. Nothing over 300 ms except large surfaces
     (`--d-duration-slow`) and springs. Nothing enters from `scale(0)`: start at 0.95 or more plus opacity.
   - Popovers and menus scale from their trigger (`transform-origin`), dialogs from the centre.
   - Things triggered from the keyboard, and things seen dozens of times a day, do not animate.
   - Springs: `transition-timing-function: var(--d-spring-default)` with `transition-duration:
     var(--d-spring-default-duration)`. Real gesture-driven motion needs a real spring in JS (interruptible,
     velocity-aware); the CSS curve is for one-shot moves.
   - `prefers-reduced-motion`: keep colour and opacity, drop movement.
9. **Text**: sentence case, no trailing full stop on labels, numbers in tabular figures (`.d-num` or
   `font-variant-numeric: tabular-nums`), data and identifiers in mono (`.d-mono`).
10. **Sizes** in `rem` tokens so the UI follows the text-size setting. Controls use `--d-size-control-*`.
11. **Corners**: tokens from `--d-radius-*`. Nested corners: inner radius = outer radius − padding.
12. **Accessible by construction**: real elements (`button`, `input`, `details`, `dialog`), labels on icon-only
    controls, text contrast at least 4.5:1, state never by colour alone.
13. **No JavaScript needed to look right.** A component must render correctly from HTML + attributes alone;
    scripts only change attributes.

## State glyphs

`<span class="d-glyph" data-glyph="task-done" role="img" aria-label="Done"></span>` Circles are work
(`task-*`), eyes are models (`model-*`). See `components/glyph.css`.

## Not built yet

Anything that is planned but not implemented is shown with the `.d-notbuilt` marker (dashed outline, the label
"Not built yet"). Never a dead control that looks live.
