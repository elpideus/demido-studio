# Demido Studio web UI

One web app for three places: the desktop shell, a browser talking to a Demido server, and the stand-alone
interactive preview with fake data. Right now it is the preview only: no core exists yet.

```bash
pnpm install
pnpm app:dev        # http://127.0.0.1:5173
pnpm app:build      # static files in apps/web/dist
pnpm app:typecheck
```

## How it is put together

| Path | Holds |
|---|---|
| `src/app/` | The shell: navigation rail, routes, the "Now" panel, toasts. `registry.ts` finds the features. |
| `src/features/<name>/index.tsx` | One feature per folder. It exports `feature`: its title, icon, route, screen and settings. |
| `src/features/settings/schema.ts` | The shape of a setting. Every setting in the app is data in this shape. |
| `src/data/types.ts` | Plain data shapes shared with the core. |
| `src/data/store.ts` | The one store, and the functions the UI calls. In the preview they are a mock core. |
| `src/data/fixtures.ts` | The fake data. |
| `src/ui/` | Thin React wrappers around the brand components. No styles of their own. |
| `src/styles/app.css` | Layout only. Look and tokens come from `brand/`. |

## Changing things

- **Add a feature:** add a folder under `src/features` with an `index.tsx` that exports `feature`. It appears in
  the navigation, gets its route, and its settings show up in Settings. Nothing else needs editing.
- **Remove a feature:** delete its folder.
- **Add a setting:** add an entry to a section in the feature's `settings` (or to `CORE_SECTIONS`). The
  Settings screen renders it, searches it and marks it when changed.
- **Change how something looks:** change it in `brand/` (tokens or a component file). The kit page and the app
  both follow.
- **Swap the fake core for the real one:** replace the functions in `src/data/store.ts`. Components do not change.

## The preview is honest about itself

Answers are scripted and say so. Anything that does nothing yet wears the "Not built yet" marker. Model
families are real; sizes, speeds, prices and licence labels are illustrative.
