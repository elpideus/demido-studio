**Neutral ramp: true neutral**

| Role | Dark | Light |
| --- | --- | --- |
| `sunken` Sunken (sidebar, wells, code) | `#0f0f0f` oklch(0.168 0.000 0) | `#f0f0f0` oklch(0.955 0.000 0) |
| `bg` App background | `#141414` oklch(0.191 0.000 0) | `#f7f7f7` oklch(0.976 0.000 0) |
| `raised` Raised surface (cards, composer) | `#1b1b1b` oklch(0.222 0.000 0) | `#fcfcfc` oklch(0.991 0.000 0) |
| `overlay` Overlay (menus, dialogs) | `#222222` oklch(0.252 0.000 0) | `#ffffff` oklch(1.000 0.000 0) |
| `fill` Interactive fill (hover, selected) | `#2b2b2b` oklch(0.289 0.000 0) | `#e7e7e7` oklch(0.928 0.000 0) |
| `hairline` Hairline border (dividers, cards) | `#343434` oklch(0.325 0.000 0) | `#dbdbdb` oklch(0.891 0.000 0) |
| `disabled` Disabled text and icons | `#575757` oklch(0.457 0.000 0) | `#a4a4a4` oklch(0.719 0.000 0) |
| `border` Strong border (control boundaries) | `#6f6f6f` oklch(0.542 0.000 0) | `#868686` oklch(0.620 0.000 0) |
| `text3` Tertiary text (pending, captions) | `#9a9a9a` oklch(0.686 0.000 0) | `#696969` oklch(0.521 0.000 0) |
| `text2` Secondary text | `#b9b9b9` oklch(0.786 0.000 0) | `#4a4a4a` oklch(0.409 0.000 0) |
| `text1` Primary text | `#e8e8e8` oklch(0.931 0.000 0) | `#1b1b1b` oklch(0.222 0.000 0) |
| `solid` Solid (logo, mono primary fill) | `#fafafa` oklch(0.985 0.000 0) | `#0e0e0e` oklch(0.164 0.000 0) |

**Accent: Magenta line**

| Token | Dark | Light |
| --- | --- | --- |
| Accent (mark and fill) | `#ff85df` oklch(0.780 0.181 338) | `#c500a5` oklch(0.560 0.246 338) |
| Hover | `#ff9fe4` oklch(0.820 0.142 338) | `#b00093` oklch(0.514 0.226 338) |
| Pressed | `#eb70cc` oklch(0.719 0.185 338) | `#9c0082` oklch(0.471 0.206 338) |
| Subtle tint background | `#351d2f` oklch(0.270 0.047 337) | `#fee7f6` oklch(0.951 0.032 339) |
| Tint border | `#6c385e` oklch(0.420 0.090 338) | `#ecb8dc` oklch(0.840 0.076 338) |
| Accent as text | `#ff91e1` oklch(0.798 0.163 338) | `#c500a5` oklch(0.560 0.246 338) |
| Text on accent fill | `#0e0e0e` (8.89:1, Lc 61.3) | `#ffffff` (5.36:1, Lc 79.6) |

**Signals used with Magenta line** (a signal marked "moved" differs from the base set in T6)

| Token | Dark | Light |
| --- | --- | --- |
| Success: text and glyph | `#35de9a` oklch(0.801 0.166 161) | `#007c5e` oklch(0.521 0.105 168) |
| Success: tint / border | `#0c2e1f` / `#095c3d` | `#dbf6eb` / `#99dbc2` |
| Success: solid fill, white text | `#00875a` oklch(0.551 0.122 161) (4.55:1) | `#008768` oklch(0.554 0.110 169) (4.50:1) |
| Caution: text and glyph | `#faca4b` oklch(0.859 0.150 88) | `#935f00` oklch(0.529 0.113 72) |
| Caution: tint / border | `#302506` / `#614a00` | `#fcecd8` / `#e9c395` |
| Caution: solid fill, white text | `#947200` oklch(0.570 0.116 88) (4.50:1) | `#9f6700` oklch(0.561 0.120 72) (4.76:1) |
| Danger: text and glyph | `#ff7269` oklch(0.720 0.174 26) | `#990012` oklch(0.430 0.175 26) |
| Danger: tint / border | `#3b1d1a` / `#773732` | `#ffe9e6` / `#f8b8b1` |
| Danger: solid fill, white text | `#d73e3c` oklch(0.590 0.190 26) (4.52:1) | `#d42229` oklch(0.560 0.210 26) (5.17:1) |
| Info: text and glyph | `#69b8fc` oklch(0.760 0.126 246) | `#4c65ca` oklch(0.541 0.159 270) (moved) |
| Info: tint / border | `#10283c` / `#1a517b` | `#e8eeff` / `#b7c9fc` |
| Info: solid fill, white text | `#287bba` oklch(0.564 0.125 246) (4.53:1) | `#516bd1` oklch(0.561 0.159 270) (4.80:1) |

**All ten pairs for Magenta line** (OKLab distance x100: normal / deuteranopia / protanopia)

| Pair | Dark | Light |
| --- | --- | --- |
| accent / success | 34.7 / 10.1 / 21.0 | 35.1 / 9.7 / 19.1 |
| accent / caution | 28.2 / 22.2 / 27.3 | 28.0 / 18.5 / 24.0 |
| accent / danger | 15.6 / 15.2 / 17.0 | 22.4 / 20.1 / 23.9 |
| accent / info | 22.4 / 7.7 / 7.1 WARN | 23.8 / 10.6 / 8.8 |
| success / caution | 19.7 / 12.6 / 8.4 | 16.3 / 9.8 / 8.9 |
| success / danger | 32.4 / 8.0 / 20.9 | 28.1 / 10.7 / 22.1 |
| success / info | 20.5 / 17.5 / 18.5 | 20.8 / 18.3 / 19.0 |
| caution / danger | 21.7 / 14.8 / 21.9 | 16.1 / 9.7 / 17.1 |
| caution / info | 28.8 / 29.7 / 26.1 | 26.9 / 27.4 / 26.4 |
| danger / info | 28.4 / 21.2 / 22.0 | 30.4 / 26.5 / 30.9 |

**Primary action: mono**

| Token | Dark | Light |
| --- | --- | --- |
| Primary fill | `#fafafa` oklch(0.985 0.000 0) | `#0e0e0e` oklch(0.164 0.000 0) |
| Primary hover / pressed | `#e6e6e6` / `#d1d1d1` | `#2a2a2a` / `#3d3d3d` |
| Text on primary | `#0e0e0e` (18.50:1, Lc 102.6) | `#ffffff` (19.30:1, Lc -107.6) |
| Focus ring | `#ff85df` oklch(0.780 0.181 338), 2 px, 2 px gap | `#c500a5` oklch(0.560 0.246 338), 2 px, 2 px gap |
