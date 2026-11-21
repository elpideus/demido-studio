# The brand lab

The tests behind the typeface and colour choices. Each folder has a report (`REPORT.md`) and the scripts
that produced its numbers and pictures.

## The comparison sheets are not in the repository

The reports name comparison sheets (`sheet-….png`, `sheets/….png`). Those are 65 images, 38 MB together.
They are rebuilt by the scripts, so they are left out to keep the repository small.

Rebuild the type sheets (about 1 minute; needs Chromium and Python):

```bash
cd brand/explorations/lab/type && pnpm install && tools/run-all.sh
```

Rebuild the colour sheets (needs Chromium):

```bash
cd brand/explorations/lab/color && node lib/selftest.mjs && node build.mjs && ./capture.sh && ./sheets.sh
```

Only Linux was captured. The reports say what that leaves unchecked.
