# Frontend stack

Written 2026-11-20, directly (the earlier research run did not reach this area). Short on purpose.

## Question

What should the one web UI be built with? It must run inside the desktop shell, in a browser against a Demido
server, and stand alone as the interactive preview. It must reach Apple-level polish, stay easy to change, and
every dependency must be compatible with GPL-3.0-or-later.

## What was checked

Versions and licences were read from the npm registry on 2026-11-20 (`pnpm view <package> version license`):

| Package | Version | Licence | Role |
|---|---|---|---|
| react, react-dom | 19.3.1 | MIT | UI library |
| typescript | 7.0.2 | Apache-2.0 | Types |
| vite | 8.3.8 | MIT | Dev server and build |
| @vitejs/plugin-react | 6.3.0 | MIT | React in Vite |
| react-router | 8.5.2 | MIT | Routing (hash, so the build runs from any folder or host) |
| @base-ui/react | 1.9.0 | MIT | Accessible unstyled parts: dialog, menu, tooltip, popover |
| zustand | 5.1.0 | MIT | Client state |
| lucide-react | 1.77.0 | ISC | Icons |
| svelte | 5.59.0 | MIT | Considered |
| solid-js | 2.0.3 | MIT | Considered |
| vue | 3.5.46 | MIT | Considered |
| motion | 14.14.0 | MIT | Considered for gesture-driven motion; not installed until used |
| lightweight-charts | 5.3.0 | Apache-2.0 | Candidate for Markets charts |
| @codemirror/view | 6.43.15 | MIT | The editor and its highlighting (DECISIONS Q5), adopted 2026-11-22 |
| mermaid | 12.3.0 | MIT | Candidate for diagrams |
| shiki | 4.6.0 | MIT | Candidate for code highlighting |
| streamdown | 2.8.0 | Apache-2.0 | Candidate for streaming markdown |

Base UI reached a stable 1.0 on 11 December 2025 and is maintained by people from Radix, Floating UI and MUI
([release notes](https://base-ui.com/react/overview/releases/v1-0-0), [InfoQ, Feb 2026](https://www.infoq.com/news/2026/02/baseui-v1-accessible)).
Its package was renamed from `@base-ui-components/react` to `@base-ui/react`; the old name stops at 1.0.0-rc.0.

Speed: published comparisons favour Svelte 5 over React 19 on high-frequency list updates
([SitePoint, Feb 2026](https://www.sitepoint.com/react-19-compiler-vs-svelte-5-virtual-dom-latency-benchmark/)).
None of them measures a streaming chat, and some are low-authority. Treated as directional only.

## Decision

React 19 with TypeScript and Vite. Base UI for the parts that are hard to make accessible. Styles come straight
from the brand kit (plain CSS and tokens), not from a CSS framework.

**Why React, not Svelte or Solid**

1. The app has more than thirty feature areas and needs many hard widgets. React has the widest choice of
   maintained, accessible, unstyled parts, and the most people who can contribute to an open project.
2. The hot path is token streaming: one text node changing many times a second. That is cheap in any of these
   libraries when the streaming message is its own component. The preview does this.
3. Bundle size, where Svelte wins, matters little inside a desktop app: the preview build is 185 KB of script,
   gzipped.
4. The risk is contained. The look lives in the brand kit's CSS, not in React components, and data arrives
   through one store. Changing the UI library later would mean rewriting thin wrappers, not the design.

**What was not adopted, and why**

- Tailwind or a CSS-in-JS library: the brand kit already defines tokens and components in plain CSS. A second
  styling system would give two sources of truth.
- A server-rendering framework: the UI is a static app served by the core. No server rendering is needed.
- React Compiler: stable (1.0.0) but not switched on yet. To be measured on the real chat before deciding.

## Risks

- TypeScript 7 and Vite 8 are recent. The preview type-checks and builds cleanly on both; if tooling around
  them proves immature, pinning one major back is a one-line change.
- Not checked: how this stack behaves in the desktop shell's web view on Linux. That belongs to the app-shell
  decision (`01-app-shell.md`), which is itself not fact-checked yet.

## Not verified

- The benchmark figures above.
- The candidate libraries for editor, charts, diagrams and markdown were only checked for version and licence,
  not evaluated.
